use druid::im::OrdMap;
use druid::kurbo::Shape;
use druid::{Data, Rect, RenderContext};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

use crate::{span_cursor, Lerp, StrokeSeq, Time, TimeDiff};

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct DrawSnippetId(pub(crate) u64);

/// A snippet is a sequence of strokes, possibly modified by a time distortion.
///
/// This struct implements [`druid::Data`]. In particular, it is cheap to clone: most of the actual
/// data lives behind shared references.
///
/// [`Lerp`]: struct.Lerp.html
/// [`druid::Data`]: ../druid/trait.Data.html
#[derive(Data, Debug, Clone)]
pub struct DrawSnippet {
    pub(crate) strokes: Arc<StrokeSeq>,
    /// The time-distortion applied to the strokes.
    pub(crate) lerp: Arc<Lerp>,
    /// The times of the strokes, with distortion applied.
    #[data(ignore)]
    times: Arc<Vec<Vec<Time>>>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub(crate) end: Option<Time>,
}

/// A collection of `DrawSnippet`s, which can be accessed using their [id].
///
/// This struct implements [`druid::Data`]. In particular, it is cheap to clone: most of the actual
/// data lives behind shared references.
///
/// [id]: struct.DrawSnippetId.html
/// [`druid::Data`]: ../druid/trait.Data.html
#[derive(Clone, Data, Default)]
pub struct DrawSnippets {
    pub(crate) last_id: u64,
    pub(crate) snippets: OrdMap<DrawSnippetId, DrawSnippet>,
}

pub type DrawCursor = span_cursor::Cursor<Time, DrawSnippetId>;

fn lerp_times(input: &StrokeSeq, lerp: &Lerp) -> Vec<Vec<Time>> {
    input
        .elts()
        .map(|stroke| stroke.times.iter().map(|t| lerp.lerp_clamped(*t)).collect())
        .collect()
}

impl DrawSnippet {
    pub fn new(strokes: StrokeSeq) -> DrawSnippet {
        if strokes.is_empty() {
            panic!("tried to create a snippet from an empty stroke sequence");
        }
        let start = strokes.first_time();
        let end = strokes.last_time();
        let lerp = Lerp::identity(start, end);
        let times = lerp_times(&strokes, &lerp);
        DrawSnippet {
            strokes: Arc::new(strokes),
            lerp: Arc::new(lerp),
            times: Arc::new(times),
            end: None,
        }
    }

    pub(crate) fn new_complete(strokes: StrokeSeq, lerp: Lerp, end: Option<Time>) -> DrawSnippet {
        let times = lerp_times(&strokes, &lerp);
        DrawSnippet {
            strokes: Arc::new(strokes),
            lerp: Arc::new(lerp),
            times: Arc::new(times),
            end,
        }
    }

    pub fn strokes<'a>(&'a self) -> impl Iterator<Item = crate::curve::StrokeRef<'a>> {
        self.strokes.strokes_with_times(&self.times[..])
    }

    /// Returns the time at which this snippet should cease to be visible, or `None` if the snippet
    /// should always be visible.
    pub fn end_time(&self) -> Option<Time> {
        self.end
    }

    pub fn with_new_lerp(&self, lerp_from: Time, lerp_to: Time) -> DrawSnippet {
        let mut lerp = (*self.lerp).clone();
        lerp.add_lerp(lerp_from, lerp_to);
        let times = lerp_times(&self.strokes, &lerp);
        DrawSnippet {
            strokes: Arc::clone(&self.strokes),
            lerp: Arc::new(lerp),
            times: Arc::new(times),
            end: self.end,
        }
    }

    pub fn key_times(&self) -> &[Time] {
        self.lerp.times()
    }

    /// Has this snippet drawn anything by `time`?
    pub fn visible_at(&self, time: Time) -> bool {
        if let Some(end) = self.end {
            self.start_time() <= time && time <= end
        } else {
            self.start_time() <= time
        }
    }

    pub fn shifted(&self, shift: TimeDiff) -> DrawSnippet {
        let lerp = self.lerp.shifted(shift);
        let times = lerp_times(&self.strokes, &lerp);
        DrawSnippet {
            strokes: Arc::clone(&self.strokes),
            lerp: Arc::new(lerp),
            times: Arc::new(times),
            end: self.end.map(|x| x + shift),
        }
    }

    pub fn start_time(&self) -> Time {
        self.times[0][0]
    }

    /// The last time at which the snippet changed.
    pub fn last_draw_time(&self) -> Time {
        *self.times.last().unwrap().last().unwrap()
    }

    pub fn render(&self, ctx: &mut impl RenderContext, time: Time) {
        if !self.visible_at(time) {
            return;
        }
        let local_time = self.lerp.unlerp_extended(time);
        self.strokes.render(ctx, local_time);
    }
}

impl DrawSnippets {
    pub fn with_new_snippet(&self, snip: DrawSnippet) -> (DrawSnippets, DrawSnippetId) {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = DrawSnippetId(ret.last_id);
        ret.snippets.insert(id, snip);
        (ret, id)
    }

    pub fn with_replacement_snippet(&self, id: DrawSnippetId, new: DrawSnippet) -> DrawSnippets {
        assert!(id.0 <= self.last_id);
        let mut ret = self.clone();
        ret.snippets.insert(id, new);
        ret
    }

    pub fn without_snippet(&self, id: DrawSnippetId) -> DrawSnippets {
        let mut ret = self.clone();
        if ret.snippets.remove(&id).is_none() {
            log::error!("tried to remove invalid snippet id {:?}", id);
        }
        ret
    }

    pub fn with_new_lerp(&self, id: DrawSnippetId, lerp_from: Time, lerp_to: Time) -> DrawSnippets {
        let snip = self.snippet(id).with_new_lerp(lerp_from, lerp_to);
        self.with_replacement_snippet(id, snip)
    }

    pub fn with_truncated_snippet(&self, id: DrawSnippetId, time: Time) -> DrawSnippets {
        let mut snip = self.snippet(id).clone();
        snip.end = Some(time);
        self.with_replacement_snippet(id, snip)
    }

    pub fn with_shifted_snippet(&self, id: DrawSnippetId, shift: TimeDiff) -> DrawSnippets {
        let snip = self.snippet(id).shifted(shift);
        self.with_replacement_snippet(id, snip)
    }

    pub fn snippet(&self, id: DrawSnippetId) -> &DrawSnippet {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (DrawSnippetId, &DrawSnippet)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }

    pub fn last_draw_time(&self) -> Time {
        self.snippets
            .values()
            .map(|snip| snip.last_draw_time())
            .max()
            .unwrap_or(Time::ZERO)
    }

    pub fn create_cursor(&self, time: Time) -> DrawCursor {
        let spans = self.snippets.iter().map(|(id, snip)| span_cursor::Span {
            start: snip.start_time(),
            end: snip.end_time(),
            id: *id,
        });
        span_cursor::Cursor::new(spans, time, time)
    }
}

impl DrawCursor {
    pub fn bboxes<'a, 'b: 'a, 'c: 'a>(
        &'b self,
        snippets: &'c DrawSnippets,
    ) -> impl Iterator<Item = Rect> + 'a {
        self.active_ids()
            .map(move |id| snippets.snippet(id))
            // TODO: if the start and end times span the snippet's end time, need to redraw the
            // whole thing. Below, we're taking this into account by returning all the individual
            // bboxes, but we could be more efficient.
            .flat_map(move |snip| {
                let (start, end) = self.current();
                // TODO: this is linear in the number of strokes, but probably most strokes will be
                // uninteresting. Using some extra cached computations in DrawSnippet, this could
                // be made (linear in useful strokes + logarithmic in total strokes).
                snip.strokes().filter_map(move |stroke| {
                    if let Some(snip_end) = snip.end_time() {
                        if self.current().0 <= snip_end && self.current().1 > snip_end {
                            return Some(
                                stroke
                                    .elements
                                    .bounding_box()
                                    .inset(stroke.style.thickness / 2.0),
                            );
                        }
                    }
                    let bbox = stroke.changes_bbox(start, end);
                    if bbox.area() == 0.0 {
                        None
                    } else {
                        Some(bbox)
                    }
                })
            })
    }
}

// The serialization of DrawSnippet is part of our save file format, so we want to keep it stable.
// Here is the stable version:
#[derive(Deserialize, Serialize)]
struct DrawSnippetSave {
    strokes: Arc<StrokeSeq>,
    lerp: Arc<Lerp>,
    end: Option<Time>,
}

impl From<DrawSnippetSave> for DrawSnippet {
    fn from(save: DrawSnippetSave) -> DrawSnippet {
        let times = lerp_times(&save.strokes, &save.lerp);
        DrawSnippet {
            strokes: save.strokes,
            lerp: save.lerp,
            times: Arc::new(times),
            end: save.end,
        }
    }
}

impl From<DrawSnippet> for DrawSnippetSave {
    fn from(snip: DrawSnippet) -> DrawSnippetSave {
        DrawSnippetSave {
            strokes: snip.strokes,
            lerp: snip.lerp,
            end: snip.end,
        }
    }
}

impl Serialize for DrawSnippet {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        DrawSnippetSave::from(self.clone()).serialize(ser)
    }
}

impl<'de> Deserialize<'de> for DrawSnippet {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<DrawSnippet, D::Error> {
        let snip: DrawSnippetSave = Deserialize::deserialize(de)?;
        Ok(snip.into())
    }
}

// The serialization of DrawSnippets is part of our save file format, and so it needs
// to remain stable. Here, we serialize DrawSnippets as an id -> DrawSnippet map.
impl Serialize for DrawSnippets {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.snippets.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for DrawSnippets {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<DrawSnippets, D::Error> {
        let map: OrdMap<DrawSnippetId, DrawSnippet> = Deserialize::deserialize(de)?;
        let max_id = map.keys().max().unwrap_or(&DrawSnippetId(0)).0;
        Ok(DrawSnippets {
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
        let snip = DrawSnippet::new(curve);
        let written = serde_cbor::to_vec(&snip).unwrap();
        let read: DrawSnippet = serde_cbor::from_slice(&written[..]).unwrap();
        assert_eq!(snip.lerp, read.lerp);
    }
}
