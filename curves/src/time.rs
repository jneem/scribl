use druid::Data;
use serde::{Deserialize, Serialize};
use std::convert::TryFrom;

/// The clock of a scribble.
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
pub struct Diff(i64);

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

    pub fn from_audio_idx(idx: i64, sample_rate: u32) -> Time {
        Time::from_micros(((idx as f64) * 1e6 / sample_rate as f64) as i64)
    }
}

impl Diff {
    /// Interpreting `self` as the offset from the beginning of an audio buffer, return the
    /// corresponding index into that buffer. Note that the return value is signed, because a
    /// negative offset from the beginning of an audio buffer corresponds to a negative index.
    ///
    /// # Examples
    ///
    /// ```
    /// use scribble_curves::time::{ZERO, Time};
    /// assert_eq!((ZERO - ZERO).as_audio_idx(44100), 0);
    /// assert_eq!((Time::from_micros(1000000) - ZERO).as_audio_idx(44100), 44100);
    /// assert_eq!((ZERO - Time::from_micros(1000000)).as_audio_idx(44100), -44100);
    /// ```
    pub fn as_audio_idx(&self, sample_rate: u32) -> isize {
        (self.0 as f64 / 1e6 * sample_rate as f64) as isize
    }

    pub fn from_audio_idx(idx: i64, sample_rate: u32) -> Diff {
        Diff(((idx as f64) * 1e6 / sample_rate as f64) as i64)
    }

    pub const fn as_micros(self) -> i64 {
        self.0
    }

    pub const fn from_micros(us: i64) -> Diff {
        Diff(us)
    }
}

impl std::ops::AddAssign<Diff> for Time {
    fn add_assign(&mut self, rhs: Diff) {
        *self = *self + rhs;
    }
}

impl std::ops::Add<Diff> for Time {
    type Output = Time;
    fn add(self, rhs: Diff) -> Time {
        Time(self.0.saturating_add(rhs.0).max(0))
    }
}

impl std::ops::Add<Diff> for Diff {
    type Output = Diff;
    fn add(self, rhs: Diff) -> Diff {
        Diff(self.0.saturating_add(rhs.0))
    }
}

impl std::ops::Sub<Diff> for Diff {
    type Output = Diff;
    fn sub(self, rhs: Diff) -> Diff {
        Diff(self.0.saturating_sub(rhs.0))
    }
}

impl std::ops::SubAssign<Diff> for Time {
    fn sub_assign(&mut self, rhs: Diff) {
        *self = *self - rhs;
    }
}

impl std::ops::Sub<Diff> for Time {
    type Output = Time;
    fn sub(self, rhs: Diff) -> Time {
        Time(self.0.saturating_sub(rhs.0).max(0))
    }
}

impl std::ops::Sub<Time> for Time {
    type Output = Diff;
    fn sub(self, rhs: Time) -> Diff {
        Diff(self.0 - rhs.0)
    }
}

impl TimeSpan {
    /// Panics if `start > end`.
    pub fn new(start: Time, end: Time) -> TimeSpan {
        assert!(start <= end);
        TimeSpan { start, end }
    }

    pub fn start(&self) -> Time {
        self.start
    }

    pub fn end(&self) -> Time {
        self.end
    }

    /// Interpolates from a time in this timespan to a time in `other`.
    /// TODO: examples,
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
