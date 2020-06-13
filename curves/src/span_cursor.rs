use std::cmp::Ord;

#[derive(Clone, Debug, PartialEq)]
pub struct Span<T: Ord + Copy, Id: Copy> {
    pub start: T,
    pub end: Option<T>,
    pub id: Id,
}

#[derive(Debug)]
pub struct Cursor<T: Ord + Copy, Id: Copy + Eq> {
    // Spans, ordered by their start times.
    spans_start: Vec<Span<T, Id>>,
    // Spans, ordered by their end times.
    spans_end: Vec<Span<T, Id>>,

    active: Vec<Span<T, Id>>,
    current: (T, T),
    next_start_idx: usize,
    next_end_idx: usize,
}

// This is the same as `Option`, but option has none before some.
// We could consider making this public and using it in `Span`.
#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum MaybeInfinite<T> {
    Finite(T),
    Infinite,
}

impl<T> From<Option<T>> for MaybeInfinite<T> {
    fn from(x: Option<T>) -> MaybeInfinite<T> {
        x.map(|y| MaybeInfinite::Finite(y))
            .unwrap_or(MaybeInfinite::Infinite)
    }
}

impl<T: PartialEq> PartialEq<T> for MaybeInfinite<T> {
    fn eq(&self, other: &T) -> bool {
        if let MaybeInfinite::Finite(ref x) = self {
            x == other
        } else {
            false
        }
    }
}

impl<T: PartialOrd> PartialOrd<T> for MaybeInfinite<T> {
    fn partial_cmp(&self, other: &T) -> Option<std::cmp::Ordering> {
        if let MaybeInfinite::Finite(ref x) = self {
            x.partial_cmp(other)
        } else {
            Some(std::cmp::Ordering::Greater)
        }
    }
}

impl<T: Ord + Copy, Id: Copy + Eq> Span<T, Id> {
    pub fn is_active(&self, time: T) -> bool {
        self.start <= time && MaybeInfinite::from(self.end) >= time
    }
}

impl<T: Ord + Copy, Id: Copy + Eq> Cursor<T, Id> {
    pub fn new<I: IntoIterator<Item = Span<T, Id>>>(
        spans: I,
        start_time: T,
        end_time: T,
    ) -> Cursor<T, Id> {
        let mut spans_start: Vec<_> = spans.into_iter().collect();
        let mut spans_end = spans_start.clone();
        spans_start.sort_by_key(|sp| sp.start);
        spans_end.sort_by_key(|sp| MaybeInfinite::from(sp.end));

        let mut active = Vec::new();
        for sp in &spans_start {
            if sp.start > end_time {
                break;
            }
            if MaybeInfinite::from(sp.end) >= MaybeInfinite::Finite(start_time) {
                active.push(sp.clone());
            }
        }

        let mut ret = Cursor {
            spans_start,
            spans_end,
            active,
            next_start_idx: 0,
            next_end_idx: 0,
            current: (start_time, end_time),
        };
        ret.reset_next_start_idx();
        ret.reset_next_end_idx();
        ret
    }

    // Resets next_start_idx to the first index with sp.start > current.1.
    fn reset_next_start_idx(&mut self) {
        let cur = self.current.1;
        match self.spans_start.binary_search_by_key(&cur, |c| c.start) {
            Ok(mut i) => {
                // We found a span starting exactly at cur, so now find the first thing starting
                // after cur.
                while i < self.spans_start.len() && self.spans_start[i].start == cur {
                    i += 1;
                }
                self.next_start_idx = i;
            }
            Err(i) => {
                self.next_start_idx = i;
            }
        }
    }

    // Resets next_end_idx to the first index with sp.end >= current.0.
    fn reset_next_end_idx(&mut self) {
        let cur = MaybeInfinite::Finite(self.current.0);
        match self
            .spans_end
            .binary_search_by_key(&cur, |sp| sp.end.into())
        {
            Ok(mut i) => {
                // We found a span ending exactly at cur, but there might be more so find the first
                // one.
                while i > 0 && MaybeInfinite::from(self.spans_end[i - 1].end) == cur {
                    i -= 1;
                }
                self.next_end_idx = i;
            }
            Err(i) => {
                self.next_end_idx = i;
            }
        }
    }

    pub fn empty(time: T) -> Cursor<T, Id> {
        Cursor {
            spans_start: Vec::new(),
            spans_end: Vec::new(),
            active: Vec::new(),
            next_start_idx: 0,
            next_end_idx: 0,
            current: (time, time),
        }
    }

    pub fn current(&self) -> (T, T) {
        self.current
    }

    pub fn advance_to(&mut self, start_time: T, end_time: T) {
        let (old_start, old_end) = self.current;
        self.current = (start_time, end_time);
        if end_time > old_end {
            while self.next_start_idx < self.spans_start.len() {
                if self.spans_start[self.next_start_idx].start <= end_time {
                    self.active
                        .push(self.spans_start[self.next_start_idx].clone());
                    self.next_start_idx += 1;
                } else {
                    break;
                }
            }
        } else {
            self.reset_next_start_idx();
        }

        if start_time < old_start {
            while self.next_end_idx > 0 {
                if MaybeInfinite::from(self.spans_end[self.next_end_idx - 1].end) >= start_time {
                    self.active
                        .push(self.spans_end[self.next_end_idx - 1].clone());
                    self.next_end_idx -= 1;
                }
            }
        } else {
            self.reset_next_end_idx();
        }

        self.active
            .retain(|sp| sp.is_active(start_time) || sp.is_active(end_time));
    }

    pub fn active_ids<'a>(&'a self) -> impl Iterator<Item = Id> + 'a {
        self.active.iter().map(|sp| sp.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(
        intervals: &[(i32, Option<i32>)],
        start_time: i32,
        end_time: i32,
    ) -> Cursor<i32, usize> {
        let spans = intervals
            .iter()
            .enumerate()
            .map(|(id, &(start, end))| Span { start, end, id });

        Cursor::new(spans, start_time, end_time)
    }

    #[test]
    fn forward() {
        let ids = |spans: &[Span<_, _>]| spans.iter().map(|sp| sp.id).collect::<Vec<_>>();

        let mut c = cursor(
            &[(0, None), (3, Some(5)), (4, Some(10)), (5, Some(7))],
            0,
            1,
        );
        assert_eq!(ids(&c.active), vec![0]);

        c.advance_to(1, 4);
        assert_eq!(ids(&c.active), vec![0, 1, 2]);

        c.advance_to(5, 6);
        assert_eq!(ids(&c.active), vec![0, 1, 2, 3]);

        c.advance_to(6, 7);
        assert_eq!(ids(&c.active), vec![0, 2, 3]);
    }
}
