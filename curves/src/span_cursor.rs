use std::cmp::Ord;
use std::hash::Hash;

#[derive(Clone, Debug, PartialEq)]
pub struct Span<T: Ord + Copy, Id: Copy + Hash> {
    pub start: T,
    pub end: Option<T>,
    pub id: Id,
}

#[derive(Debug)]
pub struct Cursor<T: Ord + Copy, Id: Copy + Eq + Hash> {
    // Spans, ordered by their start times, ascending.
    spans_start: Vec<Span<T, Id>>,
    // Spans, ordered by their end times, descending.
    spans_end: Vec<Span<T, Id>>,

    active: Vec<Span<T, Id>>,
    current: T,
    next_start_idx: usize,
    next_end_idx: usize,
}

pub struct ActiveSet<'a, T: Ord + Copy, Id: Copy + Eq + Hash> {
    cursor: &'a mut Cursor<T, Id>,
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
        x.map(|y| MaybeInfinite::Finite(y)).unwrap_or(MaybeInfinite::Infinite)
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

impl<T: Ord + Copy, Id: Copy + Eq + Hash> Span<T, Id> {
    pub fn is_active(&self, time: T) -> bool {
        self.start <= time && MaybeInfinite::from(self.end) >= time
    }
}

impl<T: Ord + Copy, Id: Copy + Eq + Hash> Cursor<T, Id> {
    pub fn new<I: IntoIterator<Item=Span<T, Id>>>(spans: I, time: T) -> Cursor<T, Id> {
        let mut spans_start: Vec<_> = spans.into_iter().collect();
        let mut spans_end = spans_start.clone();
        spans_start.sort_by_key(|sp| sp.start);
        spans_end.sort_by_key(|sp| MaybeInfinite::from(sp.end));
        spans_end.reverse();

        let mut active = Vec::new();
        for sp in &spans_start {
            if sp.start > time {
                break;
            }
            if MaybeInfinite::from(sp.end) >= MaybeInfinite::Finite(time) {
                active.push(sp.clone());
            }
        }

        let next_start_idx = spans_start.iter().position(|sp| sp.start > time).unwrap_or(spans_start.len());
        let next_end_idx = spans_end.iter().position(|sp| MaybeInfinite::from(sp.end) < time).unwrap_or(spans_end.len());

        Cursor {
            spans_start,
            spans_end,
            active,
            next_start_idx,
            next_end_idx,
            current: time,
        }
    }

    pub fn current(&self) -> T {
        self.current
    }

    pub fn advance_to(&mut self, time: T) -> ActiveSet<T, Id> {
        if time > self.current {
            while self.next_start_idx < self.spans_start.len() {
                if self.spans_start[self.next_start_idx].start <= time {
                    self.active.push(self.spans_start[self.next_start_idx].clone());
                    self.next_start_idx += 1;
                } else {
                    break;
                }
            }
        } else {
            while self.next_end_idx < self.spans_end.len() {
                if MaybeInfinite::from(self.spans_end[self.next_end_idx].end) >= time {
                    self.active.push(self.spans_end[self.next_end_idx].clone());
                    self.next_end_idx += 1;
                }
            }
        }
        self.current = time;

        ActiveSet { cursor: self }
    }

    fn finish_advance(&mut self) {
        let t = self.current;
        self.active.retain(|sp| sp.is_active(t));
    }
}

impl<'a, T: Ord + Copy, Id: Copy + Eq + Hash> Drop for ActiveSet<'a, T, Id> {
    fn drop(&mut self) {
        self.cursor.finish_advance();
    }
}

impl<'a, T: Ord + Copy, Id: Copy + Eq + Hash> ActiveSet<'a, T, Id> {
    pub fn active_spans(&self) -> impl Iterator<Item = &Span<T, Id>> {
        self.cursor.active.iter()
    }

    pub fn active_ids<'b>(&'b self) -> impl Iterator<Item = Id> + 'b {
        self.active_spans().map(|sp| sp.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cursor(intervals: &[(i32, Option<i32>)], init_time: i32) -> Cursor<i32, usize> {
        let spans = intervals.iter()
            .enumerate()
            .map(|(id, &(start, end))| Span { start, end, id });

        Cursor::new(spans, init_time)
    }

    #[test]
    fn forward() {
        let ids = |spans: &[Span<_,_>]| {
            spans.iter().map(|sp| sp.id).collect::<Vec<_>>()
        };

        let mut c = cursor(&[(0, None), (3, Some(5)), (4, Some(10)), (5, Some(7))], 0);
        assert_eq!(ids(&c.active), vec![0]);

        let next_active = c.advance_to(4).active_spans().cloned().collect::<Vec<_>>();
        assert_eq!(ids(&next_active), vec![0, 1, 2]);
        assert_eq!(ids(&c.active), vec![0, 1, 2]);

        let next_active = c.advance_to(6).active_spans().cloned().collect::<Vec<_>>();
        assert_eq!(ids(&next_active), vec![0, 1, 2, 3]);
        assert_eq!(ids(&c.active), vec![0, 2, 3]);
    }
}
