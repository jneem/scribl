use druid::Data;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// The clock of a scribl.
// This is measured in microseconds from the beginning. We enforce that the value is non-negative,
// but arithmetic is more convenient with signed types.
#[derive(
    Copy, Clone, Data, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
#[serde(transparent)]
pub struct Time(i64);

/// The difference between two [`Time`]s. Unlike `std::time::Duration`, this
/// can be negative.
#[derive(
    Copy, Clone, Data, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize,
)]
#[serde(transparent)]
pub struct TimeDiff(i64);

/// An interval of times.
#[derive(Copy, Clone, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Deserialize, Serialize)]
pub struct TimeSpan {
    start: Time,
    end: Time,
}

pub const ZERO: Time = Time(0);

impl Time {
    pub fn from_micros(us: i64) -> Time {
        assert!(us >= 0);
        Time(us)
    }

    pub fn as_micros(&self) -> i64 {
        self.0
    }

    pub fn as_gst_clock_time(&self) -> gstreamer::ClockTime {
        u64::try_from(self.as_micros()).unwrap() * gstreamer::USECOND
    }

    pub fn from_video_frame(frame: u32, fps: f64) -> Time {
        Time::from_micros((frame as f64 / fps * 1e6) as i64)
    }

    pub fn as_video_frame(&self, fps: f64) -> u32 {
        (self.0 as f64 * fps / 1e6) as u32
    }

    pub fn as_audio_idx(&self, sample_rate: u32) -> usize {
        (self.0 as f64 / 1e6 * sample_rate as f64) as usize
    }

    pub fn from_audio_idx(idx: usize, sample_rate: u32) -> Time {
        Time::from_micros(((idx as f64) * 1e6 / sample_rate as f64) as i64)
    }
}

impl TimeDiff {
    /// Interpreting `self` as the offset from the beginning of an audio buffer, return the
    /// corresponding index into that buffer. Note that the return value is signed, because a
    /// negative offset from the beginning of an audio buffer corresponds to a negative index.
    ///
    /// # Examples
    ///
    /// ```
    /// use scribl_curves::time::{ZERO, Time};
    /// assert_eq!((ZERO - ZERO).as_audio_idx(44100), 0);
    /// assert_eq!((Time::from_micros(1000000) - ZERO).as_audio_idx(44100), 44100);
    /// assert_eq!((ZERO - Time::from_micros(1000000)).as_audio_idx(44100), -44100);
    /// ```
    pub fn as_audio_idx(&self, sample_rate: u32) -> isize {
        (self.0 as f64 / 1e6 * sample_rate as f64) as isize
    }

    pub fn from_audio_idx(idx: i64, sample_rate: u32) -> TimeDiff {
        TimeDiff(((idx as f64) * 1e6 / sample_rate as f64) as i64)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }

    pub const fn from_micros(us: i64) -> TimeDiff {
        TimeDiff(us)
    }
}

impl std::ops::AddAssign<TimeDiff> for Time {
    fn add_assign(&mut self, rhs: TimeDiff) {
        *self = *self + rhs;
    }
}

impl std::ops::Add<TimeDiff> for Time {
    type Output = Time;
    fn add(self, rhs: TimeDiff) -> Time {
        Time(self.0.saturating_add(rhs.0).max(0))
    }
}

impl std::ops::Add<TimeDiff> for TimeDiff {
    type Output = TimeDiff;
    fn add(self, rhs: TimeDiff) -> TimeDiff {
        TimeDiff(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub<TimeDiff> for TimeDiff {
    type Output = TimeDiff;
    fn sub(self, rhs: TimeDiff) -> TimeDiff {
        TimeDiff(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::SubAssign<TimeDiff> for Time {
    fn sub_assign(&mut self, rhs: TimeDiff) {
        *self = *self - rhs;
    }
}

impl std::ops::Sub<TimeDiff> for Time {
    type Output = Time;
    fn sub(self, rhs: TimeDiff) -> Time {
        Time(self.0.saturating_sub(rhs.0).max(0))
    }
}

impl std::ops::Sub<Time> for Time {
    type Output = TimeDiff;
    fn sub(self, rhs: Time) -> TimeDiff {
        TimeDiff(self.0 - rhs.0)
    }
}

impl TimeSpan {
    /// Creates a new `TimeSpan` representing the closed interval `[start, end]`.
    ///
    /// # Panics
    ///
    /// Panics if `start > end`.
    pub fn new(start: Time, end: Time) -> TimeSpan {
        assert!(start <= end);
        TimeSpan { start, end }
    }

    /// The starting time of this `TimeSpan`.
    pub fn start(&self) -> Time {
        self.start
    }

    /// The ending time of this `TimeSpan`.
    pub fn end(&self) -> Time {
        self.end
    }

    /// Interpolates from a time in this timespan to a time in `other`.
    ///
    /// # Panics
    ///
    /// Panics if `time` doesn't belong to this `TimeSpan`.
    ///
    /// # Example
    /// ```rust
    /// use scribl_curves::time::{Time, TimeSpan};
    /// let me = TimeSpan::new(Time::from_micros(0), Time::from_micros(100));
    /// let other = TimeSpan::new(Time::from_micros(0), Time::from_micros(10));
    ///
    /// assert_eq!(me.interpolate_to(Time::from_micros(50), other), Time::from_micros(5));
    /// assert_eq!(me.interpolate_to(Time::from_micros(59), other), Time::from_micros(5));
    /// assert_eq!(me.interpolate_to(Time::from_micros(78), other), Time::from_micros(7));
    /// ```
    pub fn interpolate_to(&self, time: Time, other: TimeSpan) -> Time {
        assert!(self.start <= time && time <= self.end);

        if self.start == self.end {
            // If this span has length zero, it seems valid to make `time` to anything in the
            // other span. Let's just fix the end.
            return other.end;
        }

        let ratio = (time - self.start).0 as f64 / (self.end - self.start).0 as f64;
        Time(other.start.0 + ((other.end - other.start).0 as f64 * ratio) as i64)
    }
}
