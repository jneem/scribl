use druid::im::OrdMap;
use druid::kurbo::PathEl;
use druid::{Data, RenderContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::{span_cursor, Curve, Lerp, Time};

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SnippetId(u64);

#[derive(Deserialize, Serialize, Data, Debug, Clone)]
pub struct SnippetData {
    pub curve: Arc<Curve>,
    pub lerp: Arc<Lerp>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub end: Option<Time>,
}

#[derive(Clone, Data, Default)]
pub struct SnippetsData {
    last_id: u64,
    snippets: OrdMap<SnippetId, SnippetData>,
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
        let local_time = self.lerp.unlerp_extended(time);
        self.curve.render(ctx, local_time);
    }
}

impl SnippetsData {
    pub fn with_new_snippet(&self, snip: SnippetData) -> (SnippetsData, SnippetId) {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = SnippetId(ret.last_id);
        ret.snippets.insert(id, snip);
        (ret, id)
    }

    pub fn with_replacement_snippet(&self, id: SnippetId, new: SnippetData) -> SnippetsData {
        assert!(id.0 <= self.last_id);
        let mut ret = self.clone();
        ret.snippets.insert(id, new);
        ret
    }

    pub fn without_snippet(&self, id: SnippetId) -> SnippetsData {
        let mut ret = self.clone();
        if ret.snippets.remove(&id).is_none() {
            log::error!("tried to remove invalid snippet id {:?}", id);
        }
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

// The serialization of SnippetsData is part of our save file format, and so it needs
// to remain stable. Here, we serialize SnippetsData as an id -> SnippetData map.
impl Serialize for SnippetsData {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.snippets.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for SnippetsData {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<SnippetsData, D::Error> {
        let map: OrdMap<SnippetId, SnippetData> = Deserialize::deserialize(de)?;
        let max_id = map.keys().max().unwrap_or(&SnippetId(0)).0;
        Ok(SnippetsData {
            last_id: max_id,
            snippets: map,
        })
    }
}
