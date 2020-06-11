use druid::kurbo::{BezPath, ParamCurve, PathEl, PathSeg, Point};
use druid::piet::{self, LineCap, LineJoin};
use druid::{Color, RenderContext};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::effect::Effects;
use crate::time::Time;

mod serde_color {
    use super::*;

    pub fn serialize<S: Serializer>(c: &Color, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u32(c.as_rgba_u32())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Color, D::Error> {
        Ok(Color::from_rgba32_u32(u32::deserialize(de)?))
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct StrokeStyle {
    #[serde(with = "serde_color")]
    pub color: Color,
    pub thickness: f64,
    pub effects: Effects,
}

// piet::Color doesn't implement PartialEq, so we can't derive this.
impl PartialEq for StrokeStyle {
    fn eq(&self, other: &StrokeStyle) -> bool {
        self.thickness == other.thickness
            && self.color.as_rgba_u32() == other.color.as_rgba_u32()
            && self.effects == other.effects
    }
}

/// A `StrokeSeq` is a sequence of strokes, each of which is a continuous curve. Each stroke can
/// have its own style (thickness, color, effects). The strokes in a `StrokeSeq` are non-decreasing
/// in time: one stroke ends before another begins.
#[derive(Clone, Debug)]
pub struct StrokeSeq {
    path: BezPath,
    pub times: Vec<Time>,

    // The path can consist of many different "strokes" (continuous parts between
    // pen lifts). This contains the first index of each stroke, which will always
    // point to a `MoveTo`.
    stroke_boundaries: Vec<usize>,

    // This has the same length as `stroke_boundaries`.
    stroke_styles: Vec<StrokeStyle>,
}

impl StrokeSeq {
    pub fn new() -> StrokeSeq {
        StrokeSeq {
            path: BezPath::new(),
            times: Vec::new(),
            stroke_boundaries: Vec::new(),
            stroke_styles: Vec::new(),
        }
    }

    /// Returns all the elements in this `StrokeSeq`. The return value will contain only `MoveTo`
    /// (for the first element of each stroke) and `CurveTo`.
    pub fn elements(&self) -> &[PathEl] {
        self.path.elements()
    }

    fn append_path(&mut self, p: BezPath, times: Vec<Time>, style: StrokeStyle) {
        assert_eq!(p.elements().len(), times.len());
        self.stroke_boundaries.push(self.times.len());
        self.stroke_styles.push(style);
        self.times.extend_from_slice(&times[..]);
        self.path.extend(p.into_iter());
    }

    pub fn append_stroke(
        &mut self,
        points: &[Point],
        times: &[Time],
        style: StrokeStyle,
        distance_threshold: f64,
        angle_threshold: f64,
    ) {
        assert_eq!(points.len(), times.len());
        if points.is_empty() {
            return;
        }
        if let Some(&last_time) = self.times.last() {
            assert!(last_time <= times[0]);
        }

        let point_indices = crate::simplify::simplify(points, distance_threshold);
        let times: Vec<Time> = point_indices.iter().map(|&i| times[i]).collect();
        let points: Vec<Point> = point_indices.iter().map(|&i| points[i]).collect();
        let path = crate::smooth::smooth(&points, 0.4, angle_threshold);
        self.append_path(path, times, style);
    }

    pub fn strokes<'a>(&'a self) -> impl Iterator<Item = Stroke<'a>> + 'a {
        self.stroke_boundaries
            .iter()
            .enumerate()
            .map(move |(idx, &stroke_start_idx)| {
                let stroke_end_idx = self
                    .stroke_boundaries
                    .get(idx + 1)
                    .cloned()
                    .unwrap_or(self.times.len());
                Stroke {
                    style: self.stroke_styles[idx].clone(),
                    elements: &self.path.elements()[stroke_start_idx..stroke_end_idx],
                    times: &self.times[stroke_start_idx..stroke_end_idx],
                }
            })
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        let stroke_style = piet::StrokeStyle {
            line_join: Some(LineJoin::Round),
            line_cap: Some(LineCap::Round),
            ..piet::StrokeStyle::new()
        };

        for stroke in self.strokes() {
            if let Some(last) = stroke.times.last() {
                if *last <= time {
                    let color = if let Some(fade) = stroke.style.effects.fade() {
                        stroke
                            .style
                            .color
                            .with_alpha(fade.opacity_at_time(time - *last))
                    } else {
                        stroke.style.color
                    };
                    ctx.stroke_styled(
                        &stroke.elements,
                        &color,
                        stroke.style.thickness,
                        &stroke_style,
                    );
                } else {
                    // For the last stroke, we construct a new stroke whose end time is
                    // interpolated up until the current time.
                    // Note: we're doing some unnecessary cloning, just for the convenience of
                    // being able to use BezPath::get_seg.
                    let c = BezPath::from_vec(stroke.elements.to_owned());
                    let t_idx = stroke.times.binary_search(&time).unwrap_or_else(|i| i);

                    if t_idx == 0 {
                        // If we only contain the first element, it's a MoveTo and
                        // doesn't need to be drawn anyway.
                        break;
                    }

                    // We already checked that time > stroke.times.last().
                    assert!(t_idx < stroke.times.len());
                    assert_eq!(stroke.times.len(), stroke.elements.len());
                    let last_stroke = c.get_seg(t_idx).unwrap();
                    // The indexing is ok, because we already checked t_idx > 0.
                    let prev_t = stroke.times[t_idx - 1].as_micros() as f64;
                    let next_t = stroke.times[t_idx].as_micros() as f64;
                    let t_ratio = if prev_t == next_t {
                        1.0
                    } else {
                        (time.as_micros() as f64 - prev_t) / (next_t - prev_t)
                    };
                    let last_stroke = last_stroke.subsegment(0.0..t_ratio);

                    let mut c: BezPath = c.iter().take(t_idx).collect();
                    match last_stroke {
                        PathSeg::Cubic(x) => c.curve_to(x.p1, x.p2, x.p3),
                        PathSeg::Quad(x) => c.quad_to(x.p1, x.p2),
                        PathSeg::Line(x) => c.line_to(x.p1),
                    }

                    ctx.stroke_styled(
                        &c,
                        &stroke.style.color,
                        stroke.style.thickness,
                        &stroke_style,
                    );

                    // We've already rendered the stroke spanning the ending time, so we're done.
                    break;
                }
            }
        }
    }
}

// A curve gets serialized as a sequence of strokes.
impl Serialize for StrokeSeq {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut seq = ser.serialize_seq(Some(self.stroke_styles.len()))?;

        for stroke in self.strokes() {
            seq.serialize_element(&stroke)?;
        }

        seq.end()
    }
}

impl<'a> Deserialize<'a> for StrokeSeq {
    fn deserialize<D: Deserializer<'a>>(de: D) -> Result<StrokeSeq, D::Error> {
        let strokes: Vec<SavedSegment> = Deserialize::deserialize(de)?;
        let mut curve = StrokeSeq::new();

        for stroke in strokes {
            let p = |(x, y)| Point::new(x as f64 / 10_000.0, y as f64 / 10_000.0);

            let mut path = BezPath::new();
            if stroke.elements.is_empty() {
                continue;
            }
            path.move_to(p(stroke.elements[0]));
            for points in stroke.elements[1..].chunks(3) {
                path.curve_to(p(points[0]), p(points[1]), p(points[2]));
            }

            let times = stroke
                .times
                .into_iter()
                .map(|x| Time::from_micros(x as i64))
                .collect();
            curve.append_path(path, times, stroke.style);
        }

        Ok(curve)
    }
}

#[derive(Debug, Deserialize)]
struct SavedSegment {
    elements: Vec<(i32, i32)>,
    times: Vec<u64>,
    style: StrokeStyle,
}

/// A single continuous stroke in a [`StrokeSeq`], with borrowed data.
///
/// The strokes of a curve can be obtained from [`Curve::strokes`].
#[derive(Serialize)]
pub struct Stroke<'a> {
    /// The elements of this stroke. This will always start with a `PathEl::MoveTo`, and it will be
    /// followed only by `PathEl::CurveTo`s.
    #[serde(serialize_with = "serialize_path_els")]
    pub elements: &'a [PathEl],

    /// The times at which the elements were drawn. This slice has the same length as `elements`.
    pub times: &'a [Time],

    /// The style for drawing this stroke.
    pub style: StrokeStyle,
}

// We do manual serialization for curves (and strokes), mainly to ensure that
// the file format stays stable.
fn serialize_path_els<S: Serializer>(path: &[PathEl], ser: S) -> Result<S::Ok, S::Error> {
    // We serialize as a list of tuples. Each moveto gives one; each curveto gives three.
    let len: usize = path
        .iter()
        .map(|el| match el {
            PathEl::MoveTo(_) => 1,
            PathEl::CurveTo(..) => 3,
            _ => 0,
        })
        .sum();

    let mut seq = ser.serialize_seq(Some(len))?;

    let mut point = |p: &Point| -> Result<(), S::Error> {
        let x = (p.x * 10_000.0)
            .round()
            .max(i32::MIN as f64)
            .min(i32::MAX as f64) as i32;
        let y = (p.y * 10_000.0)
            .round()
            .max(i32::MIN as f64)
            .min(i32::MAX as f64) as i32;
        seq.serialize_element(&(x, y))
    };

    for el in path {
        match el {
            PathEl::MoveTo(p) => {
                point(p)?;
            }
            PathEl::CurveTo(p1, p2, p3) => {
                point(p1)?;
                point(p2)?;
                point(p3)?;
            }
            _ => {
                log::error!("error serializing: unexpected path element {:?}", el);
            }
        }
    }
    seq.end()
}

#[cfg(test)]
pub mod tests {
    use super::*;

    pub fn basic_curve() -> StrokeSeq {
        let mut c = StrokeSeq::new();
        let p = |x, y| Point::new(x, y);
        let t = |x| Time::from_micros(x);
        let style = StrokeStyle {
            color: Color::WHITE,
            thickness: 1.0,
            effects: Effects::default(),
        };
        c.append_stroke(
            &[p(0.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)],
            &[t(1), t(2), t(3)],
            style.clone(),
            0.01,
            2.0,
        );

        c.append_stroke(
            &[p(4.0, 0.0), p(1.0, 1.0), p(2.0, 2.0)],
            &[t(6), t(7), t(8)],
            style.clone(),
            0.01,
            2.0,
        );

        c
    }

    #[test]
    fn strokes() {
        let c = basic_curve();
        assert_eq!(c.strokes().count(), 2);
    }

    #[test]
    fn serialize_curve() {
        let c = basic_curve();

        let ser = serde_json::to_string(&c).unwrap();
        let deserialized: StrokeSeq = serde_json::from_str(&ser).unwrap();
        // BezPath doesn't implement PartialEq, so just compare the other parts.
        assert_eq!(deserialized.times, c.times);
        assert_eq!(deserialized.stroke_boundaries, c.stroke_boundaries);
        assert_eq!(deserialized.stroke_styles, c.stroke_styles);
    }

    #[test]
    fn serde_two_strokes() {
        let c = basic_curve();
        let written = serde_cbor::to_vec(&c).unwrap();
        let read: StrokeSeq = serde_cbor::from_slice(&written[..]).unwrap();
        assert_eq!(read.times, c.times);
        assert_eq!(read.stroke_boundaries, c.stroke_boundaries);
        assert_eq!(read.stroke_styles, c.stroke_styles);
    }
}
