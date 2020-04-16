use druid::kurbo::{BezPath, PathEl, Point};
use druid::piet::{LineCap, LineJoin, StrokeStyle};
use druid::{Color, Data, RenderContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod lerp;
pub mod span_cursor;
pub mod time;

pub use crate::lerp::Lerp;
pub use crate::time::{Diff, Time};

mod serde_path {
    use super::*;

    pub fn serialize<S: Serializer>(path: &BezPath, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&path.to_svg())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<BezPath, D::Error> {
        let s = String::deserialize(de)?;
        // TODO: once serde support appears in kurbo, drop this
        Ok(BezPath::from_svg(&s).unwrap())
    }
}

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
#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Curve {
    #[serde(with = "serde_path")]
    pub path: BezPath,
    pub times: Vec<Time>,

    // The path can consist of many different "segments" (continuous parts between
    // pen lifts). This contains the first index of each segment, which will always
    // point to a `MoveTo`.
    seg_boundaries: Vec<usize>,

    // Each segment can have a different style.
    styles: Vec<LineStyle>,
}

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SnippetId(u64);

impl Curve {
    pub fn new() -> Curve {
        Curve {
            path: BezPath::new(),
            times: Vec::new(),
            seg_boundaries: Vec::new(),
            styles: Vec::new(),
        }
    }

    pub fn line_to(&mut self, p: Point, time: Time) {
        self.path.line_to(p);
        self.times.push(time);
    }

    pub fn move_to(&mut self, p: Point, time: Time, style: LineStyle) {
        self.seg_boundaries.push(self.path.elements().len());
        self.styles.push(style);
        self.path.move_to(p);
        self.times.push(time);
    }

    pub fn segments_until<'a>(&'a self, time: Time) -> impl Iterator<Item = Segment<'a>> + 'a {
        let end_idx = match self.times.binary_search(&time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        self.seg_boundaries
            .iter()
            .enumerate()
            .map(move |(idx, &seg_start_idx)| {
                let seg_end_idx = self
                    .seg_boundaries
                    .get(idx + 1)
                    .cloned()
                    .unwrap_or(end_idx)
                    .min(end_idx)
                    .max(seg_start_idx);
                Segment {
                    style: self.styles[idx].clone(),
                    elements: &self.path.elements()[seg_start_idx..seg_end_idx],
                }
            })
            .take_while(|seg| !seg.elements.is_empty())
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        let stroke_style = StrokeStyle {
            line_join: Some(LineJoin::Round),
            line_cap: Some(LineCap::Round),
            ..StrokeStyle::new()
        };

        for seg in self.segments_until(time) {
            ctx.stroke_styled(
                &seg.elements,
                &seg.style.color,
                seg.style.thickness,
                &stroke_style,
            );
        }
    }
}

pub struct Segment<'a> {
    pub elements: &'a [PathEl],
    pub style: LineStyle,
}

#[derive(Deserialize, Serialize, Data, Debug, Clone)]
pub struct SnippetData {
    pub curve: Arc<Curve>,
    pub lerp: Arc<Lerp>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub end: Option<Time>,
}

#[derive(Deserialize, Serialize, Clone, Data, Default)]
pub struct SnippetsData {
    last_id: u64,
    snippets: Arc<BTreeMap<SnippetId, SnippetData>>,
}

pub type SnippetsCursor = span_cursor::Cursor<Time, SnippetId>;

impl SnippetData {
    // TODO: this panics if the curve is empty
    pub fn new(curve: Curve) -> SnippetData {
        let start = *curve.times.first().unwrap();
        let end = *curve.times.last().unwrap();
        let lerp = Lerp::identity(start, end);
        SnippetData {
            curve: Arc::new(curve),
            lerp: Arc::new(lerp),
            end: None,
        }
    }

    pub fn visible_at(&self, time: Time) -> bool {
        if let Some(end) = self.end {
            self.start_time() <= time && time <= end
        } else {
            self.start_time() <= time
        }
    }

    pub fn path_at(&self, time: Time) -> &[PathEl] {
        if !self.visible_at(time) {
            return &[];
        }

        // TODO: maybe there can be a better API that just gets idx directly?
        let local_time = self.lerp.unlerp_clamped(time);
        let idx = match self.curve.times.binary_search(&local_time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.curve.path.elements()[..idx]
    }

    pub fn path_between(&self, start: Time, end: Time) -> &[PathEl] {
        if let Some(my_end) = self.end {
            if start > my_end {
                return &[];
            }
        }
        if end < self.start_time() {
            return &[];
        }

        let local_start = self.lerp.unlerp_clamped(start);
        let local_end = self.lerp.unlerp_clamped(end);
        let start_idx = match self.curve.times.binary_search(&local_start) {
            Ok(i) => i,
            Err(i) => i,
        };
        let end_idx = match self.curve.times.binary_search(&local_end) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.curve.path.elements()[start_idx..end_idx]
    }

    pub fn start_time(&self) -> Time {
        self.lerp.first()
    }

    /// The last time at which the snippet changed.
    pub fn last_draw_time(&self) -> Time {
        self.lerp.last()
    }

    /// The time at which this snippet should disappear.
    pub fn end_time(&self) -> Option<Time> {
        self.end
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        if !self.visible_at(time) {
            return;
        }
        let local_time = self.lerp.unlerp_clamped(time);
        self.curve.render(ctx, local_time);
    }
}

impl SnippetsData {
    pub fn with_new_snippet(&self, snip: SnippetData) -> (SnippetsData, SnippetId) {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = SnippetId(ret.last_id);
        let mut map = (*ret.snippets).clone();
        map.insert(id, snip);
        ret.snippets = Arc::new(map);
        (ret, id)
    }

    pub fn with_replacement_snippet(&self, id: SnippetId, new: SnippetData) -> SnippetsData {
        assert!(id.0 <= self.last_id);
        let mut ret = self.clone();
        let mut map = (*ret.snippets).clone();
        map.insert(id, new);
        ret.snippets = Arc::new(map);
        ret
    }

    pub fn with_new_lerp(&self, id: SnippetId, lerp_from: Time, lerp_to: Time) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.lerp = Arc::new(snip.lerp.with_new_lerp(lerp_from, lerp_to));
        self.with_replacement_snippet(id, snip)
    }

    pub fn with_truncated_snippet(&self, id: SnippetId, time: Time) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.end = Some(time);
        self.with_replacement_snippet(id, snip)
    }

    pub fn snippet(&self, id: SnippetId) -> &SnippetData {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (SnippetId, &SnippetData)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }

    pub fn last_draw_time(&self) -> Time {
        self.snippets
            .values()
            .map(|snip| snip.last_draw_time())
            .max()
            .unwrap_or(crate::time::ZERO)
    }

    pub fn create_cursor(&self, time: Time) -> SnippetsCursor {
        let spans = self.snippets.iter().map(|(id, snip)| span_cursor::Span {
            start: snip.start_time(),
            end: snip.end_time(),
            id: *id,
        });
        span_cursor::Cursor::new(spans, time)
    }

    pub fn render_changes(
        &self,
        ctx: &mut impl RenderContext,
        cursor: &mut SnippetsCursor,
        new_time: Time,
    ) {
        //let old_time = cursor.current();
        let active_snips = cursor.advance_to(new_time);

        // TODO: we could use more precise information about the bounding boxes.
        /* This panics currently, because of kurbo #98.
        let bbox = active_snips.active_ids()
            .map(|id| self.snippets[&id].path_between(old_time, new_time).bounding_box())
            .fold(Rect::ZERO, |r1, r2| r1.union(r2));

        dbg!(bbox);
        for id in active_snips.active_ids() {
            let snip = &self.snippets[&id];
            // TODO: is there a better way to test for empty intersection?
            if snip.path_at(new_time).bounding_box().intersect(bbox).area() > 0.0 {
                snip.render(ctx, new_time);
            }
        }
        */

        for id in active_snips.active_ids() {
            let snip = &self.snippets[&id];
            snip.render(ctx, new_time);
        }
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[test]
    fn segments() {
        let mut c = Curve::new();
        let style = LineStyle {
            color: Color::WHITE,
            thickness: 1.0,
        };
        c.move_to(Point::new(0.0, 0.0), Time::from_micros(1), style.clone());
        c.line_to(Point::new(1.0, 1.0), Time::from_micros(2));
        c.line_to(Point::new(2.0, 2.0), Time::from_micros(3));

        c.move_to(Point::new(4.0, 0.0), Time::from_micros(6), style.clone());
        c.line_to(Point::new(1.0, 1.0), Time::from_micros(7));
        c.line_to(Point::new(2.0, 2.0), Time::from_micros(8));

        assert_eq!(c.segments_until(Time::from_micros(0)).count(), 0);
        assert_eq!(c.segments_until(Time::from_micros(1)).count(), 1);
        assert_eq!(
            1,
            c.segments_until(Time::from_micros(1))
                .next()
                .unwrap()
                .elements
                .len(),
        );

        assert_eq!(c.segments_until(Time::from_micros(6)).count(), 2);
        assert_eq!(
            c.segments_until(Time::from_micros(6))
                .next()
                .unwrap()
                .elements
                .len(),
            3
        );
    }
}
