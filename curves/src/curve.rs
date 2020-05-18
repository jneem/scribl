use druid::kurbo::{BezPath, ParamCurve, PathEl, PathSeg, Point};
use druid::piet::{LineCap, LineJoin, StrokeStyle};
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
pub struct LineStyle {
    #[serde(with = "serde_color")]
    pub color: Color,
    pub thickness: f64,
}

// piet::Color doesn't implement PartialEq, so we can't derive this.
impl PartialEq for LineStyle {
    fn eq(&self, other: &LineStyle) -> bool {
        self.thickness == other.thickness && self.color.as_rgba_u32() == other.color.as_rgba_u32()
    }
}

#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct SegmentData {
    pub style: LineStyle,
    pub effects: Effects,
}

impl From<LineStyle> for SegmentData {
    fn from(style: LineStyle) -> SegmentData {
        SegmentData {
            style,
            effects: Effects::default(),
        }
    }
}

// TODO: there is something strange about the API of Curve: we only allow it to
// be created as a polyline, but most of the time (and particularly for
// serializing/deserializing) we expect it to have cubic segments. That's
// because in practice we always simplify and smooth the polyline. But it would
// be better to express this using 2 different types.
#[derive(Clone, Debug)]
pub struct Curve {
    pub path: BezPath,
    pub times: Vec<Time>,

    // The path can consist of many different "segments" (continuous parts between
    // pen lifts). This contains the first index of each segment, which will always
    // point to a `MoveTo`.
    seg_boundaries: Vec<usize>,

    // This has the same length as `seg_boundaries`.
    seg_data: Vec<SegmentData>,
}

impl Curve {
    pub fn new() -> Curve {
        Curve {
            path: BezPath::new(),
            times: Vec::new(),
            seg_boundaries: Vec::new(),
            seg_data: Vec::new(),
        }
    }

    pub fn line_to(&mut self, p: Point, time: Time) {
        self.path.line_to(p);
        self.times.push(time);
    }

    pub fn move_to(&mut self, p: Point, time: Time, style: LineStyle, effects: Effects) {
        let seg_data = SegmentData { style, effects };
        self.seg_boundaries.push(self.path.elements().len());
        self.seg_data.push(seg_data);
        self.path.move_to(p);
        self.times.push(time);
    }

    pub fn append_segment(&mut self, p: BezPath, t: Vec<Time>, data: SegmentData) {
        assert_eq!(p.elements().len(), t.len());
        if t.is_empty() {
            return;
        }
        if let Some(&last_time) = self.times.last() {
            assert!(last_time <= t[0]);
        }

        self.seg_boundaries.push(self.times.len());
        self.seg_data.push(data);
        self.times.extend_from_slice(&t[..]);
        // FIXME: this is inefficient, particularly when there are many segments.
        let mut elts = self.path.elements().to_owned();
        elts.extend_from_slice(p.elements());
        self.path = BezPath::from_vec(elts);
    }

    pub fn segments<'a>(&'a self) -> impl Iterator<Item = Segment<'a>> + 'a {
        self.seg_boundaries
            .iter()
            .enumerate()
            .map(move |(idx, &seg_start_idx)| {
                let seg_end_idx = self
                    .seg_boundaries
                    .get(idx + 1)
                    .cloned()
                    .unwrap_or(self.times.len());
                Segment {
                    style: self.seg_data[idx].style.clone(),
                    effects: &self.seg_data[idx].effects,
                    elements: &self.path.elements()[seg_start_idx..seg_end_idx],
                    times: &self.times[seg_start_idx..seg_end_idx],
                }
            })
    }

    // TODO: test this. Maybe add a check_consistent function to check the invariants of `Curve`
    pub fn smoothed(&self, distance_threshold: f64, angle_threshold: f64) -> Curve {
        let mut ret = Curve::new();
        let mut path = Vec::new();
        if self.times.is_empty() {
            return ret;
        }

        for seg in self.segments() {
            let points: Vec<Point> = seg
                .elements
                .iter()
                .map(|el| match el {
                    PathEl::MoveTo(p) => *p,
                    PathEl::LineTo(p) => *p,
                    _ => panic!("can only smooth polylines"),
                })
                .collect();

            let point_indices = crate::simplify::simplify(&points, distance_threshold);
            let times: Vec<Time> = point_indices.iter().map(|&i| seg.times[i]).collect();
            let points: Vec<Point> = point_indices.iter().map(|&i| points[i]).collect();
            let curve = crate::smooth::smooth(&points, 0.4, angle_threshold);

            let seg_data = SegmentData {
                style: seg.style.clone(),
                effects: seg.effects.to_owned(),
            };
            ret.times.extend_from_slice(&times);
            ret.seg_data.push(seg_data);
            ret.seg_boundaries.push(path.len());
            path.extend_from_slice(curve.elements());
        }

        ret.path = BezPath::from_vec(path);
        ret
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        let stroke_style = StrokeStyle {
            line_join: Some(LineJoin::Round),
            line_cap: Some(LineCap::Round),
            ..StrokeStyle::new()
        };

        for seg in self.segments() {
            if let Some(last) = seg.times.last() {
                if *last <= time {
                    let color = if let Some(fade) = seg.effects.fade() {
                        if time >= *last + fade.pause + fade.fade {
                            // The curve has faded out; no need to draw it at all
                            continue;
                        } else if time >= *last + fade.pause {
                            let ratio = (time - (*last + fade.pause)).as_micros() as f64
                                / fade.fade.as_micros() as f64;
                            seg.style.color.with_alpha(1.0 - ratio)
                        } else {
                            seg.style.color
                        }
                    } else {
                        seg.style.color
                    };
                    ctx.stroke_styled(&seg.elements, &color, seg.style.thickness, &stroke_style);
                } else {
                    // For the last segment, we construct a new segment whose end time is interpolated
                    // up until the current time.
                    // Note: we're doing some unnecessary cloning, just for the convenience of being able
                    // to use BezPath::get_seg.
                    let c = BezPath::from_vec(seg.elements.to_owned());
                    let t_idx = seg.times.binary_search(&time).unwrap_or_else(|i| i);

                    if t_idx == 0 {
                        // If we only contain the first element of the curve, it's a MoveTo and doesn't
                        // need to be drawn anyway.
                        break;
                    }

                    // We already checked that time > seg.times.last().
                    assert!(t_idx < seg.times.len());
                    assert_eq!(seg.times.len(), seg.elements.len());
                    let last_seg = c.get_seg(t_idx).unwrap();
                    // The indexing is ok, because we already checked t_idx > 0.
                    let prev_t = seg.times[t_idx - 1].as_micros() as f64;
                    let next_t = seg.times[t_idx].as_micros() as f64;
                    let t_ratio = if prev_t == next_t {
                        1.0
                    } else {
                        (time.as_micros() as f64 - prev_t) / (next_t - prev_t)
                    };
                    let last_seg = last_seg.subsegment(0.0..t_ratio);

                    let mut c: BezPath = c.iter().take(t_idx).collect();
                    match last_seg {
                        PathSeg::Cubic(x) => c.curve_to(x.p1, x.p2, x.p3),
                        PathSeg::Quad(x) => c.quad_to(x.p1, x.p2),
                        PathSeg::Line(x) => c.line_to(x.p1),
                    }

                    ctx.stroke_styled(&c, &seg.style.color, seg.style.thickness, &stroke_style);

                    // We've already rendered the segment spanning the ending time, so we're done.
                    break;
                }
            }
        }
    }
}

// A curve gets serialized as a sequence of segments.
impl Serialize for Curve {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut seq = ser.serialize_seq(Some(self.seg_data.len()))?;

        for seg in self.segments() {
            seq.serialize_element(&seg)?;
        }

        seq.end()
    }
}

impl<'a> Deserialize<'a> for Curve {
    fn deserialize<D: Deserializer<'a>>(de: D) -> Result<Curve, D::Error> {
        let segments: Vec<SavedSegment> = Deserialize::deserialize(de)?;
        let mut curve = Curve::new();

        for seg in segments {
            let p = |(x, y)| Point::new(x as f64 / 10_000.0, y as f64 / 10_000.0);

            let mut path = BezPath::new();
            if seg.elements.is_empty() {
                continue;
            }
            path.move_to(p(seg.elements[0]));
            for points in seg.elements[1..].chunks(3) {
                path.curve_to(p(points[0]), p(points[1]), p(points[2]));
            }

            let times = seg
                .times
                .into_iter()
                .map(|x| Time::from_micros(x as i64))
                .collect();
            let seg_data = SegmentData {
                style: seg.style,
                effects: seg.effects,
            };
            curve.append_segment(path, times, seg_data);
        }

        Ok(curve)
    }
}

#[derive(Debug, Deserialize)]
struct SavedSegment {
    elements: Vec<(i32, i32)>,
    times: Vec<u64>,
    style: LineStyle,
    effects: Effects,
}

/// A single continuous segment of a [`Curve`], with data borrowed from the curve.
///
/// The segments of a curve can be obtained from [`Curve::segments`].
#[derive(Serialize)]
pub struct Segment<'a> {
    /// The elements of this segment. This will always start with a `PathEl::MoveTo`.
    #[serde(serialize_with = "serialize_path_els")]
    pub elements: &'a [PathEl],

    /// The times at which the elements were drawn. This slice has the same length as `elements`.
    pub times: &'a [Time],

    /// The style for drawing this segment.
    pub style: LineStyle,

    /// The effects to use for this segment.
    pub effects: &'a Effects,
}

// We do manual serialization for curves (and segments), mainly to ensure that
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

    pub fn basic_curve() -> Curve {
        let mut c = Curve::new();
        let style = LineStyle {
            color: Color::WHITE,
            thickness: 1.0,
        };
        c.move_to(
            Point::new(0.0, 0.0),
            Time::from_micros(1),
            style.clone(),
            Effects::default(),
        );
        c.line_to(Point::new(1.0, 1.0), Time::from_micros(2));
        c.line_to(Point::new(2.0, 2.0), Time::from_micros(3));

        c.move_to(
            Point::new(4.0, 0.0),
            Time::from_micros(6),
            style.clone(),
            Effects::default(),
        );
        c.line_to(Point::new(1.0, 1.0), Time::from_micros(7));
        c.line_to(Point::new(2.0, 2.0), Time::from_micros(8));

        c.smoothed(0.01, 2.0)
    }

    #[test]
    fn segments() {
        let c = basic_curve();
        assert_eq!(c.segments().count(), 2);
    }

    #[test]
    fn serialize_curve() {
        let mut c = Curve::new();
        let style = LineStyle {
            color: Color::WHITE,
            thickness: 1.0,
        };

        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        path.curve_to((0.0, 1.0), (1.0, 1.0), (1.0, 0.0));
        c.append_segment(
            path,
            vec![Time::from_micros(1), Time::from_micros(2)],
            style.into(),
        );

        let ser = serde_json::to_string(&c).unwrap();
        assert_eq!(
            &ser,
            r#"[{"elements":[[0,0],[0,10000],[10000,10000],[10000,0]],"times":[1,2],"style":{"color":4294967295,"thickness":1.0},"effects":[]}]"#
        );

        let deserialized: Curve = serde_json::from_str(&ser).unwrap();
        // BezPath doesn't implement PartialEq, so just compare the other parts.
        assert_eq!(deserialized.times, c.times);
        assert_eq!(deserialized.seg_boundaries, c.seg_boundaries);
        assert_eq!(deserialized.seg_data, c.seg_data);
    }

    #[test]
    fn serde_two_segments() {
        let c = basic_curve();
        let written = serde_cbor::to_vec(&c).unwrap();
        let read: Curve = serde_cbor::from_slice(&written[..]).unwrap();
        assert_eq!(read.times, c.times);
        assert_eq!(read.seg_boundaries, c.seg_boundaries);
        assert_eq!(read.seg_data, c.seg_data);
    }
}
