#![allow(unused_variables)]

#[derive(Clone, Debug, PartialEq)]
pub struct Lerp {
    original_values: Vec<i64>,
    lerped_values: Vec<i64>,
}

impl Lerp {
    pub fn new(original: Vec<i64>, lerped: Vec<i64>) -> Lerp {
        //assert!(original.is_sorted()); // is_sorted is nightly-only
        //assert!(lerped.is_sorted());
        assert_eq!(original.len(), lerped.len());
        Lerp {
            original_values: original,
            lerped_values: lerped,
        }
    }

    pub fn first(&self) -> i64 {
        *self.lerped_values.first().unwrap()
    }

    pub fn last(&self) -> i64 {
        *self.lerped_values.last().unwrap()
    }

    pub fn times(&self) -> &[i64] {
        &self.lerped_values
    }

    pub fn lerp(&self, t: i64) -> Option<i64> {
        use LerpResult::*;
        match lerp_interval(t, &self.original_values, &self.lerped_values) {
            AfterEnd(_) => None,
            BeforeStart(_) => None,
            SingleTime(t) => Some(t),
            Interval(t, _) => Some(t),
        }
    }

    pub fn lerp_clamped(&self, t: i64) -> i64 {
        use LerpResult::*;
        match lerp_interval(t, &self.original_values, &self.lerped_values) {
            AfterEnd(_) => self.last(),
            BeforeStart(_) => self.first(),
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn unlerp(&self, t: i64) -> Option<i64> {
        use LerpResult::*;
        match lerp_interval(t, &self.lerped_values, &self.original_values) {
            AfterEnd(_) => None,
            BeforeStart(_) => None,
            SingleTime(t) => Some(t),
            Interval(t, _) => Some(t),
        }
    }

    pub fn unlerp_clamped(&self, t: i64) -> i64 {
        use LerpResult::*;
        match lerp_interval(t, &self.lerped_values, &self.original_values) {
            AfterEnd(_) => *self.original_values.last().unwrap(),
            BeforeStart(_) => *self.original_values.first().unwrap(),
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn add_lerp(&mut self, time_from: i64, time_to: i64) {
        let local_time_from = self.unlerp_clamped(time_from);
        let idx = match self.original_values.binary_search(&local_time_from) {
            Ok(idx) => idx,
            Err(idx) => {
                self.original_values.insert(idx, local_time_from);
                self.lerped_values.insert(idx, time_from);
                idx
            }
        };
        let shift_right = time_to > self.lerped_values[idx];
        self.lerped_values[idx] = time_to;
        if shift_right {
            for v in &mut self.lerped_values[(idx + 1)..] {
                *v = (*v).max(time_to);
            }
        } else {
            for v in &mut self.lerped_values[..idx] {
                *v = (*v).min(time_to);
            }
        }
    }

    pub fn with_new_lerp(&self, time_from: i64, time_to: i64) -> Lerp {
        let mut ret = self.clone();
        ret.add_lerp(time_from, time_to);
        ret
    }
}

enum LerpResult {
    AfterEnd(i64),
    BeforeStart(i64),
    SingleTime(i64),
    Interval(i64, i64),
}

fn lerp_interval(t: i64, orig: &[i64], new: &[i64]) -> LerpResult {
    debug_assert!(orig.len() == new.len());

    if t > *orig.last().unwrap() {
        LerpResult::AfterEnd(t - orig.last().unwrap())
    } else if t < *orig.first().unwrap() {
        LerpResult::BeforeStart(t - orig.first().unwrap())
    } else {
        let (begin, end) = search_interval(t, orig);
        if new[begin] == new[end] {
            LerpResult::SingleTime(new[begin])
        } else if orig[begin] == orig[end] {
            LerpResult::Interval(new[begin], new[end])
        } else {
            debug_assert!(end == begin + 1);
            let ratio = ((t - orig[begin]) as f64) / ((orig[end] - orig[begin]) as f64);
            LerpResult::SingleTime(new[begin] + (((new[end] - new[begin]) as f64) * ratio) as i64)
        }
    }
}

// Assumes that `slice` is sorted, and that slice[0] <= x <= slice.last().unwrap().
//
// Returns a pair of indices (a, b) such that
// * slice[a] <= x
// * slice[b] >= x,
// and the interval (a, b) is the largest possible such interval.
fn search_interval(x: i64, slice: &[i64]) -> (usize, usize) {
    debug_assert!(slice[0] <= x && x <= *slice.last().unwrap());

    match slice.binary_search(&x) {
        Ok(idx) => {
            // We found one matching index, but there could be lots of them.
            let end = slice[(idx + 1)..]
                .iter()
                .position(|&y| y > x)
                .map(|i| i + idx)
                .unwrap_or(slice.len() - 1);

            let begin = slice[..idx]
                .iter()
                .rev()
                .position(|&y| y < x)
                .map(|i| idx - i)
                .unwrap_or(0);

            (begin, end)
        }
        Err(idx) => {
            // Under our assumptions above, idx must be positive, and strictly less than slice.len().
            debug_assert!(0 < idx && idx < slice.len());
            (idx - 1, idx)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_interval() {
        assert_eq!((0, 0), search_interval(1, &[1, 2, 3]));
        assert_eq!((2, 2), search_interval(3, &[1, 2, 3]));
        assert_eq!((1, 2), search_interval(3, &[1, 2, 4]));
        assert_eq!((1, 3), search_interval(1, &[0, 1, 1, 1, 2]));
        assert_eq!((1, 3), search_interval(1, &[0, 1, 1, 1]));
        assert_eq!((0, 2), search_interval(1, &[1, 1, 1, 2]));
        assert_eq!((0, 2), search_interval(1, &[1, 1, 1]));
    }

    #[test]
    fn add_lerp() {
        let lerp = Lerp::new(vec![0, 100], vec![0, 100]);

        let out = lerp.with_new_lerp(50, 80);
        assert_eq!(out.original_values, vec![0, 50, 100]);
        assert_eq!(out.lerped_values, vec![0, 80, 100]);

        let out = lerp.with_new_lerp(50, 200);
        assert_eq!(out.original_values, vec![0, 50, 100]);
        assert_eq!(out.lerped_values, vec![0, 200, 200]);

        let out = lerp.with_new_lerp(100, 150);
        assert_eq!(out.original_values, vec![0, 100]);
        assert_eq!(out.lerped_values, vec![0, 150]);

        let lerp = Lerp::new(vec![0, 100], vec![0, 200]);
        let out = lerp.with_new_lerp(100, 150);
        assert_eq!(out.original_values, vec![0, 50, 100]);
        assert_eq!(out.lerped_values, vec![0, 150, 200]);
    }
}
