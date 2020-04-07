use druid::kurbo::{BezPath, Point, PathEl, Shape, Rect};
use druid::piet::{StrokeStyle, LineCap, LineJoin};
use druid::{Color, Data, RenderContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::sync::Arc;

pub mod lerp;
pub mod span_cursor;
pub mod time;

pub use crate::time::{Diff, Time};
pub use crate::lerp::Lerp;

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
pub struct Curve {
    #[serde(with = "serde_path")]
    pub path: BezPath,
    pub times: Vec<Time>,
    #[serde(with = "serde_color")]
    pub color: Color,
    pub thickness: f64,
}

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SnippetId(u64);

impl Curve {
    pub fn new(color: &Color, thickness: f64) -> Curve {
        Curve {
            path: BezPath::new(),
            times: Vec::new(),
            color: color.clone(),
            thickness,
        }
    }
}

impl Curve {
    pub fn line_to(&mut self, p: Point, time: Time) {
        self.path.line_to(p);
        self.times.push(time);
    }

    pub fn move_to(&mut self, p: Point, time: Time) {
        self.path.move_to(p);
        self.times.push(time);
    }
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

    pub fn path_at(&self, time: Time) -> &[PathEl] {
        if let Some(end) = self.end {
            if time > end {
                return &[];
            }
        }
        if time < self.start_time() {
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
        let style = StrokeStyle {
            line_join: Some(LineJoin::Round),
            line_cap: Some(LineCap::Round),
            ..StrokeStyle::new()
        };

        ctx.stroke_styled(
            self.path_at(time),
            &self.curve.color,
            self.curve.thickness,
            &style,
        );
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
        let spans = self.snippets.iter()
            .map(|(id, snip)| span_cursor::Span { start: snip.start_time(), end: snip.end_time(), id: *id });
        span_cursor::Cursor::new(spans, time)
    }

    pub fn render_changes(&self, ctx: &mut impl RenderContext, cursor: &mut SnippetsCursor, new_time: Time) {
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
