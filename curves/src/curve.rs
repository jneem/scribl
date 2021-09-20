use druid::im::Vector;
use druid::kurbo::{BezPath, ParamCurve, PathEl, PathSeg, Point, Shape};
use druid::piet::{self, LineCap, LineJoin};
use druid::{Color, Data, Rect, RenderContext};
use serde::ser::SerializeSeq;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cell::RefCell;
use std::sync::Arc;

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

/// While drawing, this stores one continuous poly-line (from pen-down to
/// pen-up). Because we expect lots of fast changes to this, it uses interior
/// mutability to avoid repeated allocations.
#[derive(Clone, Data, Debug)]
pub struct StrokeInProgress {
    #[data(ignore)]
    pub(crate) points: Arc<RefCell<Vec<Point>>>,

    #[data(ignore)]
    pub(crate) times: Arc<RefCell<Vec<Time>>>,

    // Data comparison is done using the number of points, which grows with every modification.
    len: usize,
}

impl StrokeInProgress {
    pub fn new() -> StrokeInProgress {
        StrokeInProgress {
            points: Default::default(),
            times: Default::default(),
            len: 0,
        }
    }

    /// Adds a point, drawn at the given time (which must be at or after the previous last time).
    ///
    /// # Panics
    ///
    /// Panics if `t` is too soon.
    pub fn add_point(&mut self, p: Point, t: Time) {
        if let Some(last) = self.times.borrow().last() {
            assert!(*last <= t);
        }
        self.points.borrow_mut().push(p);
        self.times.borrow_mut().push(t);
        self.len += 1;
    }

    /// Returns the last point in the stroke.
    pub fn last_point(&self) -> Option<Point> {
        self.points.borrow().last().copied()
    }

    /// Returns true if this stroke has no points in it.
    pub fn is_empty(&self) -> bool {
        self.points.borrow().is_empty()
    }

    /// Returns the time of the first point in the stroke.
    pub fn start_time(&self) -> Option<Time> {
        self.times.borrow().first().copied()
    }

    /// Renders the part of this stroke that is visible at the time `time`.
    pub fn render(&self, ctx: &mut impl RenderContext, style: StrokeStyle, time: Time) {
        let stroke_style = piet::StrokeStyle {
            line_join: LineJoin::Round,
            line_cap: LineCap::Round,
            ..piet::StrokeStyle::new()
        };

        let ps = self.points.borrow();
        if ps.is_empty() {
            return;
        }
        let mut path = BezPath::new();
        path.move_to(ps[0]);
        for p in &ps[1..] {
            path.line_to(*p);
        }
        let last = *self.times.borrow().last().unwrap();
        let color = if let Some(fade) = style.effects.fade() {
            style.color.with_alpha(fade.opacity_at_time(time - last))
        } else {
            style.color
        };
        ctx.stroke_styled(&path, &color, style.thickness, &stroke_style);
    }

    fn to_path(&self, shape_detect: bool, distance_threshold: f64) -> Option<(BezPath, Vec<Time>)> {
        if shape_detect {
            if let Some(shape) = crate::shape_detect::detect(&self) {
                return Some((shape.path, shape.times));
            }
        }

        let points = self.points.borrow();
        let times = self.times.borrow();
        if points.is_empty() {
            return None;
        }

        let point_indices = crate::simplify::simplify(&points[..], distance_threshold);
        let times: Vec<Time> = point_indices.iter().map(|&i| times[i]).collect();
        let points: Vec<Point> = point_indices.iter().map(|&i| points[i]).collect();
        let path = crate::smooth::smooth(&points, 0.33);
        Some((path, times))
    }

    pub fn bbox(&self) -> Rect {
        if self.is_empty() {
            Rect::ZERO
        } else {
            let mut ret = Rect::from_origin_size(self.points.borrow()[0], (0.0, 0.0));
            for p in self.points.borrow().iter() {
                ret = ret.union_pt(*p);
            }
            ret
        }
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
#[derive(Clone, Data, Debug, Default)]
pub struct StrokeSeq {
    strokes: Vector<Arc<Stroke>>,
}

/// A `Stroke` consists of a single, non-empty, continuous path made up of cubic segments. Each
/// segment is annotated with the time at which it was drawn.
#[derive(Clone, Debug)]
pub struct Stroke {
    path: BezPath,
    pub(crate) times: Vec<Time>,
    style: StrokeStyle,
}

impl StrokeSeq {
    pub fn new() -> StrokeSeq {
        StrokeSeq::default()
    }

    pub fn len(&self) -> usize {
        self.strokes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.strokes.is_empty()
    }

    /// Returns time at which the first segment in the first stroke was drawn. Panics if this
    /// sequence is empty.
    pub fn first_time(&self) -> Time {
        self.strokes[0].times[0]
    }

    /// Returns time at which the last segment in the last stroke was drawn. Panics if this
    /// sequence is empty.
    pub fn last_time(&self) -> Time {
        *self.strokes.last().unwrap().times.last().unwrap()
    }

    /// If this stroke sequence ever becomes invisible (e.g. because all of the strokes in it
    /// fade out), this is the time at which it becomes invisible.
    pub fn end_time(&self) -> Option<Time> {
        self.strokes()
            .map(|stroke| {
                stroke
                    .style
                    .effects
                    .fade()
                    .map(|f| stroke.times[0] + f.pause + f.fade)
            })
            .reduce(|t1, t2| {
                if let (Some(t1), Some(t2)) = (t1, t2) {
                    Some(t1.max(t2))
                } else {
                    None
                }
            })
            .flatten()
    }

    /// Returns all the elements in this `StrokeSeq`. The return value will contain only `MoveTo`
    /// (for the first element of each stroke) and `CurveTo`.
    pub(crate) fn elts(&self) -> impl Iterator<Item = &Stroke> {
        self.strokes.iter().map(|x| x.as_ref())
    }

    pub(crate) fn append_path(&mut self, path: BezPath, times: Vec<Time>, style: StrokeStyle) {
        self.strokes
            .push_back(Arc::new(Stroke { path, times, style }));
    }

    /// Appends a `StrokeInProgress` to this stroke sequence.
    ///
    /// `distance_threshold` and `angle_threshold` are parameters that control the simplification
    /// and smoothing that we apply to the incoming points.
    ///
    /// # Panics
    ///
    /// Panics if `stroke` starts before the last existing stroke ends.
    pub fn append_stroke(
        &mut self,
        stroke: StrokeInProgress,
        style: StrokeStyle,
        shape_detect: bool,
        distance_threshold: f64,
    ) {
        if let Some((path, times)) = stroke.to_path(shape_detect, distance_threshold) {
            if !self.is_empty() {
                assert!(self.last_time() <= times[0]);
            }
            self.append_path(path, times, style);
        }
    }

    /// Returns an iterator over all the strokes in this sequence.
    pub fn strokes<'a>(&'a self) -> impl Iterator<Item = StrokeRef<'a>> + 'a {
        self.strokes.iter().map(|s| s.as_stroke_ref())
    }

    pub(crate) fn strokes_with_times<'a, 'b>(
        &'a self,
        times: &'a [Vec<Time>],
    ) -> impl Iterator<Item = StrokeRef<'a>> + 'a {
        self.strokes.iter().zip(times).map(|(s, t)| StrokeRef {
            elements: s.path.elements(),
            style: s.style.clone(),
            times: &t[..],
        })
    }

    /// Renders the part of this stroke sequence that is visible at time `time`.
    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        let stroke_style = piet::StrokeStyle {
            line_join: LineJoin::Round,
            line_cap: LineCap::Round,
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
        let mut seq = ser.serialize_seq(Some(self.strokes.len()))?;

        for stroke in self.strokes() {
            seq.serialize_element(&stroke)?;
        }

        seq.end()
    }
}

impl<'a> Deserialize<'a> for StrokeSeq {
    fn deserialize<D: Deserializer<'a>>(de: D) -> Result<StrokeSeq, D::Error> {
        let strokes: Vec<SavedSegment> = Deserialize::deserialize(de)?;
        let mut ret = StrokeSeq::new();

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
            ret.append_path(path, times, stroke.style);
        }

        Ok(ret)
    }
}

impl Stroke {
    fn as_stroke_ref<'a>(&'a self) -> StrokeRef<'a> {
        StrokeRef {
            elements: self.path.elements(),
            times: &self.times[..],
            style: self.style.clone(),
        }
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
pub struct StrokeRef<'a> {
    /// The elements of this stroke. This will always start with a `PathEl::MoveTo`, and it will be
    /// followed only by `PathEl::CurveTo`s.
    #[serde(serialize_with = "serialize_path_els")]
    pub elements: &'a [PathEl],

    /// The times at which the elements were drawn. This slice has the same length as `elements`.
    pub times: &'a [Time],

    /// The style for drawing this stroke.
    pub style: StrokeStyle,
}

impl<'a> StrokeRef<'a> {
    /// Returns a bounding box of the entire stroke.
    pub fn bbox(&self) -> Rect {
        self.elements
            .bounding_box()
            .inset(self.style.thickness / 2.0)
    }

    /// Returns a bounding box of everything that is drawn in the interval
    /// `[start_time, end_time)`.
    pub fn changes_bbox(&self, start_time: Time, end_time: Time) -> Rect {
        let start_idx = match self.times.binary_search(&start_time) {
            // binary_search gives an arbitrary match, but we want the first one.
            Ok(idx) => self.times[0..idx]
                .iter()
                .rposition(|&t| t < start_time)
                .map(|i| i + 1)
                .unwrap_or(0),
            Err(idx) => {
                if idx < self.times.len() {
                    // If the start time is in the middle of a segment, round it down to the
                    // beginning (to get a more conservative bounding box).
                    idx.saturating_sub(1)
                } else {
                    // If the start time comes after this stroke ends, we don't need to round
                    // anything.
                    idx
                }
            }
        };
        let end_idx = match self.times.binary_search(&end_time) {
            // binary_search gives an arbitrary match, but we want the first one.
            Ok(idx) => self.times[0..idx]
                .iter()
                .rposition(|&t| t < end_time)
                .map(|i| i + 1)
                .unwrap_or(0),
            Err(idx) => {
                if idx == 0 {
                    idx
                } else {
                    (idx + 1).min(self.times.len())
                }
            }
        };

        let active_elts = if let Some(fade) = self.style.effects.fade() {
            // If a fade is active between start_time and end_time, the whole stroke needs to be
            // repainted.
            let fade_start = *self.times.last().unwrap_or(&Time::ZERO) + fade.pause;
            let fade_end = fade_start + fade.fade;
            if fade_start < end_time && fade_end > start_time {
                &self.elements[..]
            } else {
                &self.elements[start_idx..end_idx]
            }
        } else {
            &self.elements[start_idx..end_idx]
        };

        let bbox = active_elts.bounding_box();
        if !active_elts.is_empty() {
            bbox.inset(self.style.thickness / 2.0)
        } else {
            Rect::ZERO
        }
    }
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
        let mut s = StrokeInProgress::new();
        s.add_point(p(0.0, 0.0), t(1));
        s.add_point(p(1.0, 1.0), t(2));
        s.add_point(p(2.0, 2.0), t(3));
        c.append_stroke(s, style.clone(), false, 0.01);

        let mut s = StrokeInProgress::new();
        s.add_point(p(4.0, 4.0), t(6));
        s.add_point(p(1.0, 1.0), t(7));
        s.add_point(p(2.0, 2.0), t(8));
        c.append_stroke(s, style.clone(), false, 0.01);

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
        for (des, orig) in deserialized.strokes.iter().zip(c.strokes.iter()) {
            assert_eq!(des.times, orig.times);
            assert_eq!(des.style, orig.style);
        }
    }

    #[test]
    fn serde_two_strokes() {
        let c = basic_curve();
        let written = serde_cbor::to_vec(&c).unwrap();
        let read: StrokeSeq = serde_cbor::from_slice(&written[..]).unwrap();
        for (des, orig) in read.strokes.iter().zip(c.strokes.iter()) {
            assert_eq!(des.times, orig.times);
            assert_eq!(des.style, orig.style);
        }
    }
}
