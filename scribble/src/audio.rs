//! This module is in charge of audio (both recording and playback). We are
//! currently using cpal, but it might be work switching to gstreamer (which
//! would be way overkill just for this module's needs, but we depend on it for
//! video encoding anyway).

use cpal::traits::{EventLoopTrait, HostTrait};
use cpal::{EventLoop, StreamData, UnknownTypeInputBuffer, UnknownTypeOutputBuffer};
use druid::im::OrdMap;
use druid::Data;
use phase_vocoder::PhaseVocoder;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};
use std::thread;

use scribble_curves::{time, Diff, Time};

/// This is in charge of the audio event loop, and various other things. There should only be one
/// of these alive at any one time, and it is intended to be long-lived (i.e., create it at startup
/// and just keep it around).
pub struct AudioState {
    event_loop: Arc<cpal::EventLoop>,
    input_device: Option<cpal::Device>,
    output_device: Option<cpal::Device>,
    format: cpal::Format,

    // These are the main ways that the audio data is synchronized with the rest of the application.
    input_data: Arc<Mutex<AudioInput>>,
    output_data: Arc<Mutex<AudioOutput>>,
}

pub const SAMPLE_RATE: u32 = 48000;

/// Each audio snippet is uniquelty identified by one of these ids.
// This is serialized as part of saving files, so its serialization format needs to remain
// stable.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub struct AudioSnippetId(u64);

/// A buffer of audio data, starting at a particular time.
///
/// The actual data is beind a pointer, so this is cheap to clone.
// This is serialized as part of saving files, so its serialization format needs to remain
// stable.
#[derive(Deserialize, Serialize, Clone, Data)]
pub struct AudioSnippetData {
    buf: Arc<Vec<i16>>,
    multiplier: f32,
    start_time: Time,
}

/// A collection of [`AudioSnippetData`](struct.AudioSnippetData.html), each one
/// identified by an [`AudioSnippetId`](struct.AudioSnippetId.html).
#[derive(Clone, Data, Default)]
pub struct AudioSnippetsData {
    last_id: u64,
    snippets: OrdMap<AudioSnippetId, AudioSnippetData>,
}

// Represents a single snippet within the cursor.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct CursorSnippet {
    id: AudioSnippetId,
    start: usize,
    end: usize,
}

/// A `Cursor` is in charge of taking a bunch of short, possibly overlapping,
/// audio buffers and presenting them as a single logical sequence of samples. It
/// does not actually store a reference to the buffers, instead working entirely
/// with indices.
///
/// A `Cursor` can either move forwards or backwards, but not both.
#[derive(Default, Debug)]
pub struct Cursor {
    cur_idx: usize,
    all_cursors: Vec<CursorSnippet>,
    next_cursor: usize,
    active_cursors: Vec<CursorSnippet>,
    forwards: bool,
}

// A convenience wrapper around the audio buffer of a snippet. This does two things:
// - it implicitly does some zero padding, and
// - it can reverse the order.
#[derive(Debug)]
struct Buf<'a> {
    inner: &'a [i16],
    offset: usize,
    len: usize,
    direction: isize,
}

impl<'a> std::ops::Index<usize> for Buf<'a> {
    type Output = i16;
    fn index(&self, idx: usize) -> &i16 {
        let dir_idx = if self.direction == 1 {
            idx
        } else {
            self.len - 1 - idx
        };

        if dir_idx >= self.offset && dir_idx < self.offset + self.inner.len() {
            &self.inner[dir_idx - self.offset]
        } else {
            &0
        }
    }
}

impl CursorSnippet {
    fn new(id: AudioSnippetId, snip: &AudioSnippetData, sample_rate: u32) -> CursorSnippet {
        let start = snip.start_time.as_audio_idx(sample_rate);
        CursorSnippet {
            id,
            start,
            end: start + snip.buf.len(),
        }
    }

    /// Gets an audio buffer from this cursor snippet. The length of the audio
    /// buffer is `amount.abs()`, and indexing the audio buffer from 0 through
    /// its length corresponds to indexing the snippet from `from` to `from +
    /// amount`. In particular, if `amount` is negative then iterating forwards
    /// through the returned buffer actually goes backwards through the audio
    /// data.
    ///
    /// The audio snippet must have a non-trivial overlap with the requested
    /// range; if not, this panics.
    fn get_buf<'a>(&mut self, data: &'a AudioSnippetsData, from: usize, amount: isize) -> Buf<'a> {
        if amount > 0 {
            debug_assert!(from < self.end);
            debug_assert!(from + amount as usize > self.start);
        } else {
            debug_assert!(from > self.start);
            debug_assert!(from < self.end + (-amount) as usize);
        }

        let snip = data.snippet(self.id);

        // The starting and ending indices relative to the buffer (could be
        // negative or extend past the buffer).
        let (start, end) = (
            from as isize - self.start as isize,
            from as isize - self.start as isize + amount,
        );
        let (start, end) = if amount > 0 {
            (start, end)
        } else {
            (end, start)
        };
        let offset = (-start).max(0) as usize;
        let start = start.max(0) as usize;
        let end = (end as usize).min(snip.buf.len());

        Buf {
            inner: &snip.buf[start..end],
            offset,
            len: amount.abs() as usize,
            direction: amount.signum(),
        }
    }

    /// If we are interested in samples between `from` and `from + amount`, does
    /// this snippet have anything to contribute?
    fn is_active(&self, from: usize, amount: isize) -> bool {
        if amount > 0 {
            from + amount as usize > self.start
        } else {
            from < self.end + (-amount) as usize
        }
    }

    /// If the audio cursor is currently at `from`, is this snippet finished
    /// contributing?
    fn is_finished(&self, from: usize, forwards: bool) -> bool {
        if forwards {
            from >= self.end
        } else {
            from <= self.start
        }
    }

    /// If the audio cursor is currently at `from`, has this snippet started
    /// contributing yet?
    fn is_started(&self, from: usize, forwards: bool) -> bool {
        if forwards {
            from >= self.start
        } else {
            from < self.end
        }
    }
}

impl Cursor {
    /// Creates a new cursor.
    ///
    /// - `snippets` are the snippets that the new cursor will curse over.
    /// - `time` gives the initial position of the cursor.
    /// - `sample_rate` is the sample rate of the audio data (TODO: maybe this
    ///     should be contained in `AudioSnippetsData`?)
    /// - `forwards` is true if the audio should be played forwards.
    ///
    /// Note that we're currently a bit wasteful when it comes to creating cursors.
    /// We don't support any kind of seeking, so we just keep creating and
    /// destroying cursors.
    pub fn new(
        snippets: &AudioSnippetsData,
        time: Time,
        sample_rate: u32,
        forwards: bool,
    ) -> Cursor {
        let mut cursors = Vec::new();
        let cur_idx = time.as_audio_idx(sample_rate);

        for (id, snip) in snippets.snippets.iter() {
            cursors.push(CursorSnippet::new(*id, snip, sample_rate));
        }
        // TODO: explain
        if forwards {
            cursors.sort_by_key(|c| c.start);
        } else {
            cursors.sort_by_key(|c| -(c.end as isize));
        }

        let mut active = Vec::new();
        let mut next_cursor = cursors.len();
        for (c_idx, c) in cursors.iter().enumerate() {
            if !c.is_started(cur_idx, forwards) {
                next_cursor = c_idx;
                break;
            }

            if !c.is_finished(cur_idx, forwards) {
                active.push(*c);
            }
        }

        Cursor {
            cur_idx,
            all_cursors: cursors,
            next_cursor,
            active_cursors: active,
            forwards,
        }
    }

    /// Fills the provided buffer with samples from the cursor, and advances the
    /// cursor past those samples.
    pub fn mix_to_buffer<B: DerefMut<Target = [i16]>>(
        &mut self,
        data: &AudioSnippetsData,
        mut buf: B,
    ) {
        // How many bytes do we need from the input buffers? This is signed: it is negative
        // if we are playing backwards.
        let input_amount = (buf.len() as isize) * if self.forwards { 1 } else { -1 };

        while self.next_cursor < self.all_cursors.len() {
            if self.all_cursors[self.next_cursor].is_active(self.cur_idx, input_amount) {
                self.active_cursors.push(self.all_cursors[self.next_cursor]);
                self.next_cursor += 1;
            } else {
                break;
            }
        }

        // TODO: we do a lot of rounding here. Maybe we should work with floats internally?
        for c in &mut self.active_cursors {
            let in_buf = c.get_buf(data, self.cur_idx, input_amount);
            let multiplier = data.snippet(c.id).multiplier;

            // TODO: we could be more efficient here, because we're potentially copying a bunch of
            // zeros from in_buf, whereas we could simply skip to the non-zero section. But it's
            // unlikely to be very expensive, whereas getting the indexing right is fiddly...
            for (idx, out_sample) in buf.iter_mut().enumerate() {
                *out_sample += ((in_buf[idx] as f32) * multiplier) as i16;
            }
        }
        if self.forwards {
            self.cur_idx += buf.len()
        } else {
            self.cur_idx = self.cur_idx.saturating_sub(buf.len());
        }
        let cur_idx = self.cur_idx;
        let forwards = self.forwards;
        self.active_cursors
            .retain(|c| !c.is_finished(cur_idx, forwards));
    }

    /// Has this cursor finished producing non-zero samples?
    pub fn is_finished(&self) -> bool {
        self.active_cursors.is_empty() && self.next_cursor == self.all_cursors.len()
    }
}

impl AudioState {
    /// Initializes the audio and spawns the audio thread. Returns an object that can be used
    /// to control the audio.
    pub fn init() -> AudioState {
        let host = cpal::default_host();
        let event_loop = host.event_loop();
        let input_device = host.default_input_device();
        let output_device = host.default_output_device();
        let format = cpal::Format {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE as u32),
            data_type: cpal::SampleFormat::I16,
        };

        if input_device.is_none() {
            log::error!("failed to open an input audio device");
        }
        if output_device.is_none() {
            log::error!("failed to open an output audio device");
        }

        let ret = AudioState {
            event_loop: Arc::new(event_loop),
            input_device,
            output_device,
            format,
            input_data: Arc::new(Mutex::new(AudioInput::default())),
            output_data: Arc::new(Mutex::new(AudioOutput::default())),
        };

        let event_loop = Arc::clone(&ret.event_loop);
        let input_data = Arc::clone(&ret.input_data);
        let output_data = Arc::clone(&ret.output_data);
        thread::spawn(move || audio_thread(event_loop, input_data, output_data));
        ret
    }

    pub fn set_velocity(&mut self, vel: f64) {
        self.output_data.lock().unwrap().speed_factor = vel;
    }

    pub fn start_recording(&mut self) -> anyhow::Result<()> {
        if let Some(ref input_device) = self.input_device {
            let input_stream = self
                .event_loop
                .build_input_stream(input_device, &self.format)?;

            {
                let mut input = self.input_data.lock().unwrap();
                assert!(input.id.is_none());
                input.id = Some(input_stream.clone());
                input.buf.clear();
            }

            self.event_loop.play_stream(input_stream)?;
        }
        Ok(())
    }

    pub fn stop_recording(&mut self) -> Vec<i16> {
        let mut input_data = self.input_data.lock().unwrap();
        if let Some(id) = input_data.id.take() {
            self.event_loop.destroy_stream(id);
        } else {
            log::error!("no input stream while stopping recording");
        }

        let mut buf = Vec::new();
        std::mem::swap(&mut input_data.buf, &mut buf);

        process_audio(buf)
    }

    pub fn start_playing(
        &mut self,
        data: AudioSnippetsData,
        time: Time,
        velocity: f64,
    ) -> anyhow::Result<()> {
        if let Some(ref output_device) = self.output_device {
            let cursor = Cursor::new(&data, time, SAMPLE_RATE, velocity > 0.0);
            let output_stream = self
                .event_loop
                .build_output_stream(output_device, &self.format)?;

            {
                let mut output = self.output_data.lock().unwrap();
                assert!(output.id.is_none());
                output.id = Some(output_stream.clone());
                output.bufs = data;
                output.speed_factor = velocity;
                output.cursor = cursor;
            }

            self.event_loop.play_stream(output_stream)?;
        }
        Ok(())
    }

    pub fn stop_playing(&mut self) {
        let mut output = self.output_data.lock().unwrap();
        if let Some(id) = output.id.take() {
            self.event_loop.destroy_stream(id);
        } else {
            log::error!("tried to stop a non-existent stream");
        }
    }
}

impl AudioSnippetData {
    pub fn new(buf: Vec<i16>, start_time: Time) -> AudioSnippetData {
        AudioSnippetData {
            buf: Arc::new(buf),
            multiplier: 1.0,
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
        let length = time::Diff::from_audio_idx(self.buf().len() as i64, SAMPLE_RATE);
        self.start_time() + length
    }

    /// Normalize this audio signal to have a target loudness.
    pub fn set_multiplier(&mut self, target_lufs: f32) {
        let orig_lufs = lufs::loudness(self.buf.iter().map(|&x| (x as f32) / (i16::MAX as f32)));
        self.multiplier = lufs::multiplier(orig_lufs, target_lufs)
    }

    pub fn multiplier(&self) -> f32 {
        self.multiplier
    }
}

impl AudioSnippetsData {
    pub fn with_new_snippet(&self, snip: AudioSnippetData) -> AudioSnippetsData {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = AudioSnippetId(ret.last_id);
        ret.snippets.insert(id, snip);
        ret
    }

    pub fn without_snippet(&self, id: AudioSnippetId) -> AudioSnippetsData {
        let mut ret = self.clone();
        ret.snippets.remove(&id);
        ret
    }

    pub fn snippet(&self, id: AudioSnippetId) -> &AudioSnippetData {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (AudioSnippetId, &AudioSnippetData)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }

    pub fn end_time(&self) -> Time {
        self.snippets
            .values()
            .map(|snip| snip.end_time())
            .max()
            .unwrap_or(time::ZERO)
    }
}

#[derive(Default)]
struct AudioInput {
    id: Option<cpal::StreamId>,
    buf: Vec<i16>,
}

#[derive(Default)]
struct AudioOutput {
    id: Option<cpal::StreamId>,
    speed_factor: f64,
    cursor: Cursor,
    bufs: AudioSnippetsData,
}

fn audio_thread(
    event_loop: Arc<EventLoop>,
    input: Arc<Mutex<AudioInput>>,
    output: Arc<Mutex<AudioOutput>>,
) {
    let mut pvoc = PhaseVocoder::new(1.0);
    let mut pvoc_speed = 1.0f64;
    let mut mix_buffer = vec![0; 2048];

    // Keep track of the last output stream, because when the output
    // stream changes then we need to clear the vocoder's buffer.
    let mut last_output_stream_id = None;

    event_loop.run(move |stream_id, stream_data| {
        let stream_data = stream_data.expect("error getting stream data");
        match stream_data {
            StreamData::Output {
                buffer: UnknownTypeOutputBuffer::I16(mut buf),
            } => {
                if last_output_stream_id.as_ref() != Some(&stream_id) {
                    pvoc.reset(pvoc_speed.abs() as f32);
                    last_output_stream_id = Some(stream_id.clone());
                }
                let mut buf: &mut [i16] = &mut *buf;

                for elem in buf.iter_mut() {
                    *elem = 0;
                }

                // We do mix + time-shifting until the output buffer is full.
                while !buf.is_empty() {
                    for elem in &mut mix_buffer {
                        *elem = 0;
                    }

                    {
                        // We do the cheaper part (mixing from the various buffers into our single buffer)
                        // in a smaller scope, to avoid holding the lock.
                        let mut output_data = output.lock().unwrap();
                        let output_data = output_data.deref_mut();
                        if output_data.id.as_ref() != Some(&stream_id) {
                            return;
                        }
                        output_data
                            .cursor
                            .mix_to_buffer(&output_data.bufs, &mut mix_buffer[..]);
                        if output_data.speed_factor != pvoc_speed {
                            pvoc_speed = output_data.speed_factor;
                            pvoc.reset(pvoc_speed.abs() as f32);
                        }
                    }

                    // Now that we've dropped the lock, do the time-shifting and actually write to the buffer.
                    pvoc.input(&mix_buffer[..]);
                    let len = pvoc.samples_available().min(buf.len());
                    pvoc.consume_output(&mut buf[..len]);
                    buf = &mut buf[len..];
                }
            }
            StreamData::Input {
                buffer: UnknownTypeInputBuffer::I16(buf),
            } => {
                let mut input_data = input.lock().unwrap();
                if input_data.id != Some(stream_id) {
                    return;
                }
                input_data.buf.extend_from_slice(&buf);
            }
            _ => {
                panic!("unexpected data");
            }
        }
    });
}

// Processes the recorded audio.
// - Truncates the beginning and end a little bit (to remove to sound of the user pressing the keyboard to start/stop recording).
// - Runs noise removal using RNNoise.
const TRUNCATION_LEN: Diff = Diff::from_micros(100_000);
fn process_audio(mut buf: Vec<i16>) -> Vec<i16> {
    let trunc_samples = TRUNCATION_LEN.as_audio_idx(SAMPLE_RATE) as usize;
    if buf.len() <= 4 * trunc_samples {
        return Vec::new();
    }
    // We truncate the end of the buffer, but instead of truncating the beginning we set
    // it all to zero (because if we truncate it, it messes with the synchronization between
    // audio and animation).
    for i in 0..trunc_samples {
        buf[i] = 0;
    }

    // Truncate the buffer and convert it to f32 also, since that's what RNNoise wants.
    let buf_end = buf.len() - trunc_samples;
    let mut float_buf: Vec<f32> = buf[..buf_end].iter().map(|x| *x as f32).collect();
    // Do some fade-in and fade-out.
    for i in 0..trunc_samples {
        let factor = i as f32 / trunc_samples as f32;
        float_buf[trunc_samples + i] *= factor;
        float_buf[buf_end - 1 - i] *= factor;
    }

    // RNNoise likes the input to be a multiple of FRAME_SIZE.
    let fs = rnnoise_c::FRAME_SIZE;
    let new_size = ((float_buf.len() + (fs - 1)) / fs) * fs;
    float_buf.resize(new_size, 0.0);
    let mut out_buf = vec![0.0f32; float_buf.len()];
    let mut state = rnnoise_c::DenoiseState::new();
    for (in_chunk, out_chunk) in float_buf.chunks_exact(fs).zip(out_buf.chunks_exact_mut(fs)) {
        state.process_frame_mut(in_chunk, out_chunk);
    }
    out_buf.into_iter().map(|x| x as i16).collect()
}

// Here is the serialization for audio. Note that the serialization format needs to remain
// stable, because it is used for file saving.
//
// Specifically, we serialize the audio state as a map id -> snippet data. Any other fields
// on `AudioSnippetsData` are ignored, and must be reconstituted from the snippet map on
// deserialization.
impl Serialize for AudioSnippetsData {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        self.snippets.serialize(ser)
    }
}

impl<'de> Deserialize<'de> for AudioSnippetsData {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<AudioSnippetsData, D::Error> {
        let snips: OrdMap<AudioSnippetId, AudioSnippetData> = Deserialize::deserialize(de)?;
        let max_id = snips.keys().max().unwrap_or(&AudioSnippetId(0)).0;
        Ok(AudioSnippetsData {
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
                let mut ret = AudioSnippetsData::default();
                $(
                    let buf: &[i16] = $buf;
                    let time = Time::from_micros($time * 1000000);
                    ret = ret.with_new_snippet(AudioSnippetData::new(buf.to_owned(), time));
                )*

                ret
            }
        }
    }

    #[test]
    fn forward() {
        let snips = snips!(0 => &[1, 2, 3, 4, 5]);
        // a sample rate of 1 is silly, but it lets us get the indices right without any rounding issues.
        let mut c = Cursor::new(&snips, time::ZERO, 1, true);
        let mut out = vec![0; 5];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn forward_offset() {
        let snips = snips!(5 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, time::ZERO, 1, true);
        let mut out = vec![0; 15];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn backward() {
        let snips = snips!(2 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, Time::from_micros(9 * 1000000), 1, false);
        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![0, 0, 5, 4, 3, 2, 1, 0, 0, 0]);
    }

    #[test]
    fn backward_already_finished() {
        let snips = snips!(0 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, Time::from_micros(0), 1, false);
        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            2 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, time::ZERO, 1, true);
        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets_backwards() {
        let snips = snips!(
            0 => &[1, 2, 3],
            2 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, Time::from_micros(10 * 1000000), 1, false);
        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![0, 0, 0, 0, 0, 3, 2, 4, 2, 1]);
    }

    #[test]
    fn non_overlapping_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            12 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, time::ZERO, 1, true);
        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 0, 0, 0, 0, 0, 0, 0]);

        let mut out = vec![0; 10];
        c.mix_to_buffer(&snips, &mut out[..]);
        assert_eq!(out, vec![0, 0, 1, 2, 3, 0, 0, 0, 0, 0]);
    }
}
