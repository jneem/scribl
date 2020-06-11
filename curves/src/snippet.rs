use druid::im::OrdMap;
use druid::kurbo::PathEl;
use druid::{Data, RenderContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::{span_cursor, Lerp, StrokeSeq, Time};

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct SnippetId(u64);

/// A snippet is a sequence of strokes, each one possibly with a time distortion (provided by
/// [`Lerp`]).
///
/// This struct implements [`druid::Data`]. In particular, it is cheap to clone: most of the actual
/// data lives behind shared references.
///
/// [`Lerp`]: struct.Lerp.html
/// [`druid::Data`]: ../druid/trait.Data.html
#[derive(Deserialize, Serialize, Data, Debug, Clone)]
pub struct SnippetData {
    #[serde(rename = "curve")]
    strokes: Arc<StrokeSeq>,
    lerp: Arc<Lerp>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    end: Option<Time>,
}

/// A collection of `SnippetData`s, which can be accessed using their [id].
///
/// This struct implements [`druid::Data`]. In particular, it is cheap to clone: most of the actual
/// data lives behind shared references.
///
/// [id]: struct.SnippetId.html
/// [`druid::Data`]: ../druid/trait.Data.html
#[derive(Clone, Data, Default)]
pub struct SnippetsData {
    last_id: u64,
    snippets: OrdMap<SnippetId, SnippetData>,
}

pub type SnippetsCursor = span_cursor::Cursor<Time, SnippetId>;

impl SnippetData {
    pub fn new(strokes: StrokeSeq) -> SnippetData {
        if strokes.times.is_empty() {
            panic!("tried to create a snippet from an empty stroke sequence");
        }
        let start = *strokes.times.first().unwrap();
        let end = *strokes.times.last().unwrap();
        let lerp = Lerp::identity(start, end);
        SnippetData {
            strokes: Arc::new(strokes),
            lerp: Arc::new(lerp),
            end: None,
        }
    }

    pub fn strokes<'a>(&'a self) -> impl Iterator<Item = crate::curve::Stroke<'a>> {
        self.strokes.strokes()
    }

    /// Returns the `Lerp` object used for time-distorting this snippet.
    pub fn lerp(&self) -> &Lerp {
        &self.lerp
    }

    /// Returns the time at which this snippet should cease to be visible, or `None` if the snippet
    /// should always be visible.
    pub fn end_time(&self) -> Option<Time> {
        self.end
    }

    /// Has this snippet drawn anything by `time`?
    pub fn visible_at(&self, time: Time) -> bool {
        if let Some(end) = self.end {
            self.start_time() <= time && time <= end
        } else {
            self.start_time() <= time
        }
    }

    /// Returns a reference to just that part of the path that exists up until `time`.
    ///
    /// If there if a path element that starts before `time` and ends after `time`, it will be
    /// included too.
    pub fn path_at(&self, time: Time) -> &[PathEl] {
        if !self.visible_at(time) {
            return &[];
        }

        let local_time = self.lerp.unlerp_clamped(time);
        let idx = match self.strokes.times.binary_search(&local_time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.strokes.elements()[..idx]
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
        let start_idx = match self.strokes.times.binary_search(&local_start) {
            Ok(i) => i,
            Err(i) => i,
        };
        let end_idx = match self.strokes.times.binary_search(&local_end) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.strokes.elements()[start_idx..end_idx]
    }

    pub fn start_time(&self) -> Time {
        self.lerp.first()
    }

    /// The last time at which the snippet changed.
    pub fn last_draw_time(&self) -> Time {
        self.lerp.last()
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        if !self.visible_at(time) {
            return;
        }
        let local_time = self.lerp.unlerp_extended(time);
        self.strokes.render(ctx, local_time);
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
            .unwrap_or(Time::ZERO)
    }

    pub fn create_cursor(&self, time: Time) -> SnippetsCursor {
        let spans = self.snippets.iter().map(|(id, snip)| span_cursor::Span {
            start: snip.start_time(),
            end: snip.end_time(),
            id: *id,
        });
        span_cursor::Cursor::new(spans, time)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_snippet() {
        let curve = crate::curve::tests::basic_curve();
        let snip = SnippetData::new(curve);
        let written = serde_cbor::to_vec(&snip).unwrap();
        let read: SnippetData = serde_cbor::from_slice(&written[..]).unwrap();
        assert_eq!(snip.lerp, read.lerp);
    }
}
