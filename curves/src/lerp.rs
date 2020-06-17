use serde::{Deserialize, Serialize};

use crate::time::{Time, TimeDiff, TimeSpan};

/// Specifies interpolations between two sets of times.
///
/// This struct maintains two lists of "key-frame" times, which it uses for mapping times from one
/// scale to another.  The two lists of key-frames must have the same length; the `i`th key-frame
/// of one list is mapped to the `i`th key-frame of the other list, and times that fall in between
/// key-frames are mapped using linear interpolation. There are various different ways that you can
/// handle times that lie outside the range of key-frames, as illustrated in the examples below.
///
/// # Examples
///
/// ```
/// # use scribl_curves::{Lerp, Time};
///
/// let t = |x| Time::from_micros(x);
///
/// // Create an "identity" lerp that just maps the interval [100, 200] to itself.
/// let mut lerp = Lerp::identity(t(100), t(200));
///
/// // Now map time 150 to time 180. The interval [100, 150] will be stretched to cover
/// // [100, 180], and the interval [150, 200] will be squished to [180, 200].
/// lerp.add_lerp(t(150), t(180));
///
/// // The midpoint between 100 and 150 will be mapped to the midpoint between 100 and 180.
/// assert_eq!(lerp.lerp(t(125)), Some(t(140)));
/// // The midpoint between 150 and 200 will be mapped to the midpoint between 180 and 200.
/// assert_eq!(lerp.lerp(t(175)), Some(t(190)));
///
/// // Adding a lerp is like taking the *composition* of the two mappings. So taking the previous
/// // lerp and then sending 140 to 150 means that the original 125 will get sent to 150.
/// lerp.add_lerp(t(140), t(150));
/// assert_eq!(lerp.lerp(t(125)), Some(t(150)));
/// assert_eq!(lerp.lerp(t(120)), Some(t(140)));
///
/// // When we map a time to something outside the current output range, something special happens:
/// // all of the output times will get "squished" to the new time. Currently, the interval
/// // [125, 200] is getting mapped to [150, 200]; after this line, that whole interval will get
/// // mapped to 250.
/// lerp.add_lerp(t(150), t(250));
/// assert_eq!(lerp.lerp(t(125)), Some(t(250)));
/// assert_eq!(lerp.lerp(t(199)), Some(t(250)));
///
/// // There are three different ways that elements outside the domain can be handled.
/// // The `lerp` method returns `None` if asked to map something outside its domain.
/// assert_eq!(lerp.lerp(t(201)), None);
/// // The `lerp_clamped` method effectively rounds its input to the nearest end of the domain.
/// assert_eq!(lerp.lerp_clamped(t(201)), t(250));
/// // The `lerp_extended` method extends the map past the original domain by linear interpolation.
/// assert_eq!(lerp.lerp_extended(t(201)), t(251));
/// ```
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Lerp {
    pub(crate) original_values: Vec<Time>,
    pub(crate) lerped_values: Vec<Time>,
}

impl Lerp {
    /// Creates a new `Lerp` in which the key-frames in `original` are mapped to the key-frames in
    /// `lerped`. Both `original` and `lerped` must be sorted, and they must have the same length.
    ///
    /// # Panics
    ///
    /// Panics unless `original` and `lerped` have the same length.
    fn new(original: Vec<Time>, lerped: Vec<Time>) -> Lerp {
        //assert!(original.is_sorted()); // is_sorted is nightly-only
        //assert!(lerped.is_sorted());
        assert_eq!(original.len(), lerped.len());
        Lerp {
            original_values: original,
            lerped_values: lerped,
        }
    }

    /// Creates a new `Lerp` representing the identity mapping on the interval `[start, end]`.
    ///
    /// That is, `start` will be mapped to `start`, `end` will be mapped to `end`, and everything
    /// in between will also be mapped to itself. Times outside the interval `[start, end]` will be
    /// treated differently depending on which of the lerping functions you call.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use scribl_curves::{Lerp, Time};
    /// let t = |x| Time::from_micros(x);
    /// assert_eq!(Lerp::identity(t(0), t(10)).lerp(t(5)), Some(t(5)));
    /// assert_eq!(Lerp::identity(t(0), t(10)).lerp(t(10)), Some(t(10)));
    /// ```
    pub fn identity(start: Time, end: Time) -> Lerp {
        assert!(end >= start);
        Lerp::new(vec![start, end], vec![start, end])
    }

    /// The first time in the range of the mapping.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use scribl_curves::{Lerp, Time};
    /// let t = |x| Time::from_micros(x);
    /// assert_eq!(
    ///     Lerp::identity(t(10), t(20))
    ///         .with_new_lerp(t(10), t(9))
    ///         .first(),
    ///     t(9)
    /// );
    /// ```
    pub fn first(&self) -> Time {
        *self.lerped_values.first().unwrap()
    }

    /// The last time in the range of the mapping.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use scribl_curves::{Lerp, Time};
    /// let t = |x| Time::from_micros(x);
    /// assert_eq!(
    ///     Lerp::identity(t(10), t(20))
    ///         .with_new_lerp(t(20), t(25))
    ///         .last(),
    ///     t(25)
    /// );
    /// ```
    pub fn last(&self) -> Time {
        *self.lerped_values.last().unwrap()
    }

    /// All of the key frames in the range of the mapping.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use scribl_curves::{Lerp, Time};
    /// let t = |x| Time::from_micros(x);
    /// assert_eq!(
    ///     Lerp::identity(t(10), t(20))
    ///         .with_new_lerp(t(15), t(18))
    ///         .times(),
    ///     &[t(10), t(18), t(20)]
    /// );
    /// ```
    pub fn times(&self) -> &[Time] {
        &self.lerped_values
    }

    pub fn lerp(&self, t: Time) -> Option<Time> {
        use LerpResult::*;
        match lerp_interval(t, &self.original_values, &self.lerped_values) {
            AfterEnd(_) => None,
            BeforeStart(_) => None,
            SingleTime(t) => Some(t),
            Interval(t, _) => Some(t),
        }
    }

    pub fn lerp_clamped(&self, t: Time) -> Time {
        use LerpResult::*;
        match lerp_interval(t, &self.original_values, &self.lerped_values) {
            AfterEnd(_) => self.last(),
            BeforeStart(_) => self.first(),
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn lerp_extended(&self, t: Time) -> Time {
        use LerpResult::*;
        match lerp_interval(t, &self.original_values, &self.lerped_values) {
            AfterEnd(t) => *self.lerped_values.last().unwrap() + t,
            BeforeStart(t) => *self.lerped_values.first().unwrap() + t,
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn unlerp(&self, t: Time) -> Option<Time> {
        use LerpResult::*;
        match lerp_interval(t, &self.lerped_values, &self.original_values) {
            AfterEnd(_) => None,
            BeforeStart(_) => None,
            SingleTime(t) => Some(t),
            Interval(t, _) => Some(t),
        }
    }

    pub fn unlerp_clamped(&self, t: Time) -> Time {
        use LerpResult::*;
        match lerp_interval(t, &self.lerped_values, &self.original_values) {
            AfterEnd(_) => *self.original_values.last().unwrap(),
            BeforeStart(_) => *self.original_values.first().unwrap(),
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn unlerp_extended(&self, t: Time) -> Time {
        use LerpResult::*;
        match lerp_interval(t, &self.lerped_values, &self.original_values) {
            AfterEnd(t) => *self.original_values.last().unwrap() + t,
            BeforeStart(t) => *self.original_values.first().unwrap() + t,
            SingleTime(t) => t,
            Interval(t, _) => t,
        }
    }

    pub fn add_lerp(&mut self, time_from: Time, time_to: Time) {
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

    pub fn with_new_lerp(&self, time_from: Time, time_to: Time) -> Lerp {
        let mut ret = self.clone();
        ret.add_lerp(time_from, time_to);
        ret
    }
}

enum LerpResult {
    AfterEnd(TimeDiff),
    BeforeStart(TimeDiff),
    SingleTime(Time),
    Interval(Time, Time),
}

fn lerp_interval(t: Time, orig: &[Time], new: &[Time]) -> LerpResult {
    debug_assert!(orig.len() == new.len());

    if t > *orig.last().unwrap() {
        LerpResult::AfterEnd(t - *orig.last().unwrap())
    } else if t < *orig.first().unwrap() {
        LerpResult::BeforeStart(t - *orig.first().unwrap())
    } else {
        let (begin, end) = search_interval(t, orig);
        if new[begin] == new[end] {
            LerpResult::SingleTime(new[begin])
        } else if orig[begin] == orig[end] {
            LerpResult::Interval(new[begin], new[end])
        } else {
            debug_assert!(end == begin + 1);
            let orig_span = TimeSpan::new(orig[begin], orig[end]);
            let new_span = TimeSpan::new(new[begin], new[end]);
            LerpResult::SingleTime(orig_span.interpolate_to(t, new_span))
        }
    }
}

// Assumes that `slice` is sorted, and that slice[0] <= x <= slice.last().unwrap().
//
// Returns a pair of indices (a, b) such that
// * slice[a] <= x
// * slice[b] >= x,
// and the interval (a, b) is the largest possible such interval.
fn search_interval(x: Time, slice: &[Time]) -> (usize, usize) {
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
        fn search(x: i64, xs: &[i64]) -> (usize, usize) {
            let xs: Vec<_> = xs.iter().cloned().map(Time::from_micros).collect();
            search_interval(Time::from_micros(x), &xs)
        }
        assert_eq!((0, 0), search(1, &[1, 2, 3]));
        assert_eq!((2, 2), search(3, &[1, 2, 3]));
        assert_eq!((1, 2), search(3, &[1, 2, 4]));
        assert_eq!((1, 3), search(1, &[0, 1, 1, 1, 2]));
        assert_eq!((1, 3), search(1, &[0, 1, 1, 1]));
        assert_eq!((0, 2), search(1, &[1, 1, 1, 2]));
        assert_eq!((0, 2), search(1, &[1, 1, 1]));
    }

    fn t(x: i64) -> Time {
        Time::from_micros(x)
    }
    macro_rules! tvec {
        [$($t:tt)*] => {
            vec![$($t)*].into_iter().map(Time::from_micros).collect::<Vec<_>>()
        }
    }

    #[test]
    fn add_lerp() {
        let lerp = Lerp::new(tvec![0, 100], tvec![0, 100]);

        let out = lerp.with_new_lerp(t(50), t(80));
        assert_eq!(out.original_values, tvec![0, 50, 100]);
        assert_eq!(out.lerped_values, tvec![0, 80, 100]);

        let out = lerp.with_new_lerp(t(50), t(200));
        assert_eq!(out.original_values, tvec![0, 50, 100]);
        assert_eq!(out.lerped_values, tvec![0, 200, 200]);

        let out = lerp.with_new_lerp(t(100), t(150));
        assert_eq!(out.original_values, tvec![0, 100]);
        assert_eq!(out.lerped_values, tvec![0, 150]);

        let lerp = Lerp::new(tvec![0, 100], tvec![0, 200]);
        let out = lerp.with_new_lerp(t(100), t(150));
        assert_eq!(out.original_values, tvec![0, 50, 100]);
        assert_eq!(out.lerped_values, tvec![0, 150, 200]);
    }

    #[test]
    fn unlerp() {
        let lerp = Lerp::new(tvec![1, 101], tvec![201, 301]);
        assert_eq!(lerp.unlerp(t(201)), Some(t(1)));
        assert_eq!(lerp.unlerp(t(301)), Some(t(101)));
        assert_eq!(lerp.unlerp(t(200)), None);
        assert_eq!(lerp.unlerp(t(302)), None);

        assert_eq!(lerp.unlerp_clamped(t(200)), t(1));
        assert_eq!(lerp.unlerp_clamped(t(302)), t(101));

        assert_eq!(lerp.unlerp_extended(t(200)), t(0));
        assert_eq!(lerp.unlerp_extended(t(199)), t(0));
        assert_eq!(lerp.unlerp_extended(t(302)), t(102));
    }
}
