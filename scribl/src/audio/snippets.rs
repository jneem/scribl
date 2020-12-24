use druid::im::OrdMap;
use druid::Data;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use scribl_curves::{Cursor, Span, Time, TimeDiff};

use super::SAMPLE_RATE;

/// Each audio snippet is uniquely identified by one of these ids.
// This is serialized as part of saving files, so its serialization format needs to remain
// stable.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct TalkSnippetId(u64);

/// A buffer of audio data, starting at a particular time.
///
/// The actual data is beind a pointer, so this is cheap to clone.
// This is serialized as part of saving files, so its serialization format needs to remain
// stable.
#[derive(Deserialize, Serialize, Clone, Data, PartialEq)]
pub struct TalkSnippet {
    buf: Arc<Vec<i16>>,
    multiplier: f32,
    start_time: Time,
}

/// A collection of [`TalkSnippet`](struct.TalkSnippet.html), each one
/// identified by an [`TalkSnippetId`](struct.TalkSnippetId.html).
#[derive(Clone, Data, Default, PartialEq)]
pub struct TalkSnippets {
    last_id: u64,
    snippets: OrdMap<TalkSnippetId, TalkSnippet>,
}

impl TalkSnippet {
    pub fn new(buf: Vec<i16>, start_time: Time, multiplier: f32) -> TalkSnippet {
        TalkSnippet {
            buf: Arc::new(buf),
            multiplier,
            start_time,
        }
    }

    pub fn buf(&self) -> &[i16] {
        &self.buf
    }

    pub fn start_time(&self) -> Time {
        self.start_time
    }

    pub fn end_time(&self) -> Time {
        let length = TimeDiff::from_audio_idx(self.buf().len() as i64, SAMPLE_RATE);
        self.start_time() + length
    }

    pub fn shifted(&self, shift: TimeDiff) -> TalkSnippet {
        TalkSnippet {
            buf: Arc::clone(&self.buf),
            multiplier: self.multiplier,
            start_time: self.start_time + shift,
        }
    }

    pub fn multiplier(&self) -> f32 {
        self.multiplier
    }

    pub fn multiplied(&self, factor: f32) -> TalkSnippet {
        TalkSnippet {
            buf: Arc::clone(&self.buf),
            multiplier: self.multiplier * factor,
            start_time: self.start_time,
        }
    }

    fn idx(&self, time: Time) -> usize {
        (time - self.start_time())
            .as_audio_idx(SAMPLE_RATE)
            .max(0)
            .min(self.buf.len() as isize) as usize
    }

    /// Returns a new snippet, with all audio between `from` and `to` silenced.
    pub fn silenced(&self, from: Time, to: Time) -> TalkSnippet {
        let from_idx = self.idx(from);
        let to_idx = self.idx(to);
        let (from_idx, to_idx) = (from_idx.min(to_idx), from_idx.max(to_idx));

        if from_idx < to_idx {
            let mut buf = self.buf.deref().clone();
            for i in from_idx..to_idx {
                buf[i] = 0;
            }
            TalkSnippet {
                buf: Arc::new(buf),
                ..self.clone()
            }
        } else {
            self.clone()
        }
    }

    /// Returns a new (shorter) snippet, with all audio between `from` and `to` deleted.
    pub fn snipped(&self, from: Time, to: Time) -> TalkSnippet {
        let from_idx = self.idx(from);
        let to_idx = self.idx(to);
        let (from_idx, to_idx) = (from_idx.min(to_idx), from_idx.max(to_idx));

        if from_idx < to_idx {
            let mut buf = self.buf.deref().clone();
            buf.drain(from_idx..to_idx);
            TalkSnippet {
                buf: Arc::new(buf),
                ..self.clone()
            }
        } else {
            self.clone()
        }
    }
}

impl TalkSnippets {
    pub fn with_new_snippet(&self, snip: TalkSnippet) -> (TalkSnippets, TalkSnippetId) {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = TalkSnippetId(ret.last_id);
        ret.snippets.insert(id, snip);
        (ret, id)
    }

    pub fn has_snippet(&self, snip: TalkSnippetId) -> bool {
        self.snippets.contains_key(&snip)
    }

    fn with_modified_snippet(
        &self,
        id: TalkSnippetId,
        f: impl FnOnce(&TalkSnippet) -> TalkSnippet,
    ) -> TalkSnippets {
        let mut ret = self.clone();
        let snip = f(ret.snippet(id));
        ret.snippets.insert(id, snip);
        ret
    }

    pub fn with_shifted_snippet(&self, id: TalkSnippetId, shift: TimeDiff) -> TalkSnippets {
        self.with_modified_snippet(id, |s| s.shifted(shift))
    }

    pub fn with_multiplied_snippet(&self, id: TalkSnippetId, factor: f64) -> TalkSnippets {
        self.with_modified_snippet(id, |s| s.multiplied(factor as f32))
    }

    pub fn with_silenced_snippet(&self, id: TalkSnippetId, start: Time, end: Time) -> TalkSnippets {
        self.with_modified_snippet(id, |s| s.silenced(start, end))
    }

    pub fn with_snipped_snippet(&self, id: TalkSnippetId, start: Time, end: Time) -> TalkSnippets {
        let ret = self.with_modified_snippet(id, |s| s.snipped(start, end));
        if ret.snippet(id).buf.is_empty() {
            self.without_snippet(id)
        } else {
            ret
        }
    }

    pub fn without_snippet(&self, id: TalkSnippetId) -> TalkSnippets {
        let mut ret = self.clone();
        ret.snippets.remove(&id);
        ret
    }

    pub fn snippet(&self, id: TalkSnippetId) -> &TalkSnippet {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (TalkSnippetId, &TalkSnippet)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }

    pub fn end_time(&self) -> Time {
        self.snippets
            .values()
            .map(|snip| snip.end_time())
            .max()
            .unwrap_or(Time::ZERO)
    }

    /// Fills the provided buffer with samples from the cursor, and advance the cursor.
    pub fn mix_to<B: DerefMut<Target = [i16]>>(
        &self,
        cursor: &Cursor<usize, TalkSnippetId>,
        mut buf: B,
    ) {
        for sp in cursor.active_spans() {
            let buf: &mut [i16] = &mut buf;
            let snip = self.snippet(sp.id);
            let multiplier = snip.multiplier;

            let (curs_start, curs_end) = cursor.current();
            let snip_start = curs_start.saturating_sub(sp.start);
            let snip_end = curs_end.saturating_sub(sp.start).min(snip.buf.len());
            let buf_offset = sp.start.saturating_sub(curs_start);

            for (idx, sample) in snip.buf[snip_start..snip_end].iter().enumerate() {
                buf[buf_offset + idx] += (*sample as f32 * multiplier) as i16;
            }
        }
    }

    pub fn snippet_spans<'a>(&'a self) -> impl Iterator<Item = Span<usize, TalkSnippetId>> + 'a {
        self.snippets.iter().map(|(&id, snip)| {
            let start = snip.start_time().as_audio_idx(SAMPLE_RATE);
            let end = start + snip.buf.len();
            Span {
                start,
                end: Some(end),
                id,
            }
        })
    }
}

// Here is the serialization for audio. Note that the serialization format needs to remain
// stable, because it is used for file saving.
//
// Specifically, we serialize the audio state as a map id -> snippet data. Any other fields
// on `TalkSnippets` are ignored, and must be reconstituted from the snippet map on
// deserialization.
impl Serialize for TalkSnippets {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.snippets.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for TalkSnippets {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<TalkSnippets, D::Error> {
        let snips: OrdMap<TalkSnippetId, TalkSnippet> = Deserialize::deserialize(de)?;
        let max_id = snips.keys().max().unwrap_or(&TalkSnippetId(0)).0;
        Ok(TalkSnippets {
            snippets: snips,
            last_id: max_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! snips {
        ($($time:expr => $buf:expr),*) => {
            {
                let mut ret = TalkSnippets::default();
                $(
                    let buf: &[i16] = $buf;
                    let time = Time::from_audio_idx($time, SAMPLE_RATE);
                    ret = ret.with_new_snippet(TalkSnippet::new(buf.to_owned(), time, 1.0));
                )*

                ret
            }
        }
    }

    #[test]
    fn forward() {
        let snips = snips!(0 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(snips.snippet_spans(), 0, 0);
        let mut out = vec![0; 5];
        c.advance_to(0, 5);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn forward_offset() {
        let snips = snips!(5 => &[1, 2, 3, 4, 5]);
        let c = Cursor::new(snips.snippet_spans(), 0, 15);
        let mut out = vec![0; 15];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![0, 0, 0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn backward() {
        let snips = snips!(3 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(snips.snippet_spans(), 9, 9);
        let mut out = vec![0; 10];
        c.advance_to(0, 10);
        snips.mix_to(&c, &mut out[..]);
        // 2 leading zeros, not 3, because of rounding in audio/index conversion.
        assert_eq!(out, vec![0, 0, 1, 2, 3, 4, 5, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            3 => &[1, 2, 3]
        );
        let c = Cursor::new(snips.snippet_spans(), 0, 10);
        let mut out = vec![0; 10];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets_backwards() {
        let snips = snips!(
            0 => &[1, 2, 3],
            3 => &[1, 2, 3]
        );
        let mut c = Cursor::new(snips.snippet_spans(), 10, 20);
        let mut out = vec![0; 10];
        c.advance_to(0, 10);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_overlapping_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            12 => &[1, 2, 3]
        );
        let mut c = Cursor::new(snips.snippet_spans(), 0, 10);
        let mut out = vec![0; 10];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 0, 0, 0, 0, 0, 0, 0]);

        let mut out = vec![0; 10];
        c.advance_to(10, 20);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![0, 0, 1, 2, 3, 0, 0, 0, 0, 0]);
    }
}
