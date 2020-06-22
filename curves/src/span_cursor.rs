use std::cmp::Ord;

#[derive(Clone, Debug, PartialEq)]
pub struct Span<T: Ord + Copy, Id: Copy> {
    pub start: T,
    pub end: Option<T>,
    pub id: Id,
}

/// A cursor allows for efficiently scanning through a collection of overlapping intervals (which
/// we call "spans"). It is optimized for the case where you need to repeatedly move the current
/// position by a little bit in either direction; in this case, the complexity is O(n),
/// where `n` is the number of "active" spans that overlap the times you're interested in.
#[derive(Debug)]
pub struct Cursor<T: Ord + Copy, Id: Copy + Eq> {
    // Spans, ordered by their start times.
    spans_start: Vec<Span<T, Id>>,
    // Spans, ordered by their end times.
    spans_end: Vec<Span<T, Id>>,

    // The set of active spans (unordered). This is the set of spans that have a non-zero
    // intersection with the current interval.
    active: Vec<Span<T, Id>>,

    // An interval of times, inclusive of both ends.
    current: (T, T),

    // The index (into `spans_start`) of the first element having `start > current.1`
    next_start_idx: usize,
    // The index (into `span_end`) of the first element with `end >= current.0`. Note that this is
    // an active element (unlike with next_start_idx).
    next_end_idx: usize,
}

// This is the same as `Option`, but option has none before some. We could consider making this
// public and using it in `Span`.
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
    /// A span is active if its time interval overlaps with `[start_time, end_time]`.
    pub fn is_active(&self, start_time: T, end_time: T) -> bool {
        self.start <= end_time && MaybeInfinite::from(self.end) >= start_time
    }
}

impl<T: Ord + Copy, Id: Copy + Eq> Cursor<T, Id> {
    /// Creates a new cursor for the given set of spans, and initializes its current position to
    /// be the interval `[start_time, end_time]` (inclusive of both ends).
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

        let next_start_idx = spans_start
            .iter()
            .position(|sp| sp.start > end_time)
            .unwrap_or(spans_start.len());
        let next_end_idx = spans_end
            .iter()
            .position(|sp| MaybeInfinite::from(sp.end) >= start_time)
            .unwrap_or(spans_end.len());

        Cursor {
            spans_start,
            spans_end,
            active,
            next_start_idx,
            next_end_idx,
            current: (start_time, end_time),
        }
    }

    /// Creates a cursor with no spans, set to the time interval `[time, time]`.
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

    /// Returns this cursors current time interval.
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
            let before = self.spans_start[0..self.next_start_idx]
                .iter()
                .rposition(|sp| sp.start <= end_time);
            self.next_start_idx = before.map(|x| x + 1).unwrap_or(0);
        }

        if start_time < old_start {
            while self.next_end_idx > 0 {
                if MaybeInfinite::from(self.spans_end[self.next_end_idx - 1].end) >= start_time {
                    self.active
                        .push(self.spans_end[self.next_end_idx - 1].clone());
                    self.next_end_idx -= 1;
                } else {
                    break;
                }
            }
        } else {
            self.next_end_idx = self.spans_end[self.next_end_idx..]
                .iter()
                .position(|sp| MaybeInfinite::from(sp.end) >= start_time)
                .map(|x| x + self.next_end_idx)
                .unwrap_or_else(|| self.spans_end.len());
        }

        self.active.retain(|sp| sp.is_active(start_time, end_time));
    }

    pub fn active_ids<'a>(&'a self) -> impl Iterator<Item = Id> + 'a {
        self.active.iter().map(|sp| sp.id)
    }

    pub fn active_spans<'a>(&'a self) -> impl Iterator<Item = Span<T, Id>> + 'a {
        self.active.iter().cloned()
    }

    pub fn is_finished(&self) -> bool {
        self.active.is_empty() && self.next_start_idx == self.spans_start.len()
    }

    #[cfg(test)]
    fn assert_invariants(&self) {
        for s in &self.spans_start {
            assert_eq!(
                s.is_active(self.current.0, self.current.1),
                self.active.contains(s)
            );
        }

        assert_eq!(
            self.next_start_idx,
            self.spans_start
                .iter()
                .position(|s| s.start > self.current.1)
                .unwrap_or(self.spans_start.len())
        );

        assert_eq!(
            self.next_end_idx,
            self.spans_end
                .iter()
                .position(|s| MaybeInfinite::from(s.end) >= self.current.0)
                .unwrap_or(self.spans_end.len())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

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

    #[test]
    fn backward() {
        let ids = |spans: &[Span<_, _>]| spans.iter().map(|sp| sp.id).collect::<Vec<_>>();

        let mut c = cursor(
            &[(0, None), (3, Some(5)), (4, Some(10)), (5, Some(7))],
            11,
            11,
        );
        assert_eq!(ids(&c.active), vec![0]);

        c.advance_to(10, 11);
        assert_eq!(ids(&c.active), vec![0, 2]);

        c.advance_to(8, 12);
        assert_eq!(ids(&c.active), vec![0, 2]);

        c.advance_to(6, 8);
        assert_eq!(ids(&c.active), vec![0, 2, 3]);

        c.advance_to(0, 2);
        assert_eq!(ids(&c.active), vec![0]);

        c.advance_to(0, 0);
        assert_eq!(ids(&c.active), vec![0]);
    }

    #[test]
    fn tmp_test() {
        let mut cursor = cursor(&[(0, Some(95)), (0, Some(0))], 1, 1);
        cursor.assert_invariants();
        dbg!(&cursor);
        cursor.advance_to(1, 1);
        dbg!(&cursor);
        cursor.assert_invariants();
    }

    proptest! {
        #[test]
        fn check_invariants(
            spans in prop::collection::vec((0usize..100, 0usize..100), 1..20),
            start in (0usize..100, 0usize..150),
            moves in prop::collection::vec((0usize..100, 0usize..150), 1..10)
        ) {
            let spans = spans.into_iter().enumerate().map(|(i, (x, y))| Span {
                start: x.min(y),
                end: Some(x.max(y)),
                id: i,
            });
            let mut cursor = Cursor::new(spans, start.0.min(start.1), start.0.max(start.1));
            cursor.assert_invariants();

            for (a, b) in moves {
                cursor.advance_to(a.min(b), a.max(b));
                cursor.assert_invariants();
            }
        }
    }
}
