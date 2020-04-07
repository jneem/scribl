use std::collections::HashMap;
use std::hash::Hash;

use scribble_curves::{time, SnippetData, SnippetId, Time};

use crate::audio::{AudioSnippetData, AudioSnippetId};

pub struct SnippetBounds<T> {
    start: Time,
    end: Option<Time>,
    id: T,
}

pub struct SnippetLayout<T> {
    pub positions: HashMap<T, usize>,
    pub num_rows: usize,
}

impl From<(SnippetId, &SnippetData)> for SnippetBounds<SnippetId> {
    fn from(data: (SnippetId, &SnippetData)) -> SnippetBounds<SnippetId> {
        SnippetBounds {
            start: data.1.lerp.first(),
            end: data.1.end,
            id: data.0,
        }
    }
}

impl From<(AudioSnippetId, &AudioSnippetData)> for SnippetBounds<AudioSnippetId> {
    fn from(data: (AudioSnippetId, &AudioSnippetData)) -> SnippetBounds<AudioSnippetId> {
        SnippetBounds {
            start: data.1.start_time(),
            end: Some(data.1.end_time()),
            id: data.0,
        }
    }
}

pub fn layout<Id: Copy + Hash + Eq, T: Into<SnippetBounds<Id>>, I: Iterator<Item = T>>(
    iter: I,
) -> SnippetLayout<Id> {
    let mut bounds: Vec<SnippetBounds<Id>> = iter.map(|t| t.into()).collect();
    bounds.sort_by_key(|b| b.start);

    let mut row_ends = Vec::<Option<Time>>::new();
    let mut ret = SnippetLayout {
        positions: HashMap::new(),
        num_rows: 0,
    };
    'bounds: for b in &bounds {
        for (row_idx, end) in row_ends.iter_mut().enumerate() {
            if let Some(finite_end_time) = *end {
                if finite_end_time == time::ZERO || b.start > finite_end_time {
                    *end = b.end;
                    ret.positions.insert(b.id, row_idx);
                    continue 'bounds;
                }
            }
        }
        // We couldn't fit the snippet, so add a new row.
        ret.positions.insert(b.id, row_ends.len());
        ret.num_rows += 1;
        row_ends.push(b.end);
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    // Creates a snippet that is empty, but has a starting and (possibly) an ending time.
    fn snip(id: usize, start: Time, end: Option<Time>) -> SnippetBounds<usize> {
        SnippetBounds { start, end, id }
    }

    macro_rules! snips {
        ( $(($begin:expr, $end:expr)),* ) => {
            {
                let mut ret = Vec::<SnippetBounds<usize>>::new();
                let mut id = 0;
                $(
                    id += 1;
                    ret.push(snip(id, Time::from_micros($begin), $end.map(Time::from_micros)));
                )*
                ret.into_iter()
            }
        }
    }

    #[test]
    fn layout_infinite() {
        let snips = snips!((0, None), (1, None));
        let layout = layout(snips);
        assert_eq!(layout.positions[&1], 0);
        assert_eq!(layout.positions[&2], 1);
    }
}
