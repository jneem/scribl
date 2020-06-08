//! This module is in charge of audio (both recording and playback).

use anyhow::{anyhow, Result};
use druid::im::OrdMap;
use druid::Data;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

use scribl_curves::{time, Diff, Time};

/// This is in charge of the audio event loop, and various other things. There should only be one
/// of these alive at any one time, and it is intended to be long-lived (i.e., create it at startup
/// and just keep it around).
pub struct AudioState {
    output_data: Arc<Mutex<OutputData>>,
    // The pipeline will be `None` if there was an error while creating it. In that case, we
    // already printed an error message so we'll just silently (heh) not play any audio.
    output_pipeline: Option<gst::Pipeline>,
    input_data: Arc<Mutex<InputData>>,
    input_pipeline: Option<gst::Pipeline>,
}

/// This data is shared between the main program and the gstreamer playback pipeline (as
/// represented by the `needs_data` callback on its `app-src` element). It is protected by
/// a mutex. When the main program wants to, say, update the audio data, it unlocks the mutex and
/// mutates `snips`.
pub struct OutputData {
    pub snips: AudioSnippetsData,
    pub cursor: Cursor,
    pub forwards: bool,
}

struct InputData {
    buf: Vec<i16>,
}

pub const SAMPLE_RATE: u32 = 48000;

/// Each audio snippet is uniquely identified by one of these ids.
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
#[derive(Default, Debug)]
pub struct Cursor {
    start_idx: usize,
    end_idx: usize,

    // A cursor for every snippet, sorted by start index (increasing).
    forward_cursors: Vec<CursorSnippet>,
    // The index in forward_cursors of the first snippet starting after `end_idx`.
    next_forward_cursor: usize,

    // A cursor for every snippet, sorted by end index (decreasing).
    backward_cursors: Vec<CursorSnippet>,
    // The index in backward_cursors of the first snippet ending before `start_idx`.
    next_backward_cursor: usize,

    // The set of cursors for which `cur_idx` is in the interval [start, end).
    active_cursors: Vec<CursorSnippet>,
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
    pub fn new(snippets: &AudioSnippetsData, time: Time, sample_rate: u32) -> Cursor {
        let mut forward_cursors = Vec::new();
        let cur_idx = time.as_audio_idx(sample_rate);

        for (id, snip) in snippets.snippets.iter() {
            forward_cursors.push(CursorSnippet::new(*id, snip, sample_rate));
        }

        let mut backward_cursors = forward_cursors.clone();
        forward_cursors.sort_by_key(|c| c.start);
        backward_cursors.sort_by_key(|c| -(c.end as isize));

        let active_cursors = forward_cursors
            .iter()
            .copied()
            .filter(|c| c.start < cur_idx && cur_idx < c.end)
            .collect();

        let mut ret = Cursor {
            start_idx: cur_idx,
            end_idx: cur_idx,
            forward_cursors,
            next_forward_cursor: 0,
            backward_cursors,
            next_backward_cursor: 0,
            active_cursors,
        };
        ret.reset_next_forward_cursor();
        ret.reset_next_backward_cursor();
        ret
    }

    fn reset_next_forward_cursor(&mut self) {
        match self
            .forward_cursors
            .binary_search_by_key(&self.end_idx, |c| c.start)
        {
            Ok(mut i) => {
                // We found a cursor starting at exactly end_idx, but there might be more, so find
                // the first one.
                while i > 0 && self.forward_cursors[i - 1].start == self.end_idx {
                    i -= 1;
                }
                self.next_forward_cursor = i;
            }
            Err(i) => {
                self.next_forward_cursor = i;
            }
        }
    }

    fn reset_next_backward_cursor(&mut self) {
        match self
            .backward_cursors
            .binary_search_by_key(&-(self.start_idx as isize), |c| -(c.end as isize))
        {
            Ok(mut i) => {
                // We found a cursor ending exactly at start_idx, but there might be more so find
                // the first one.
                while i > 0 && self.backward_cursors[i - 1].end == self.start_idx {
                    i -= 1;
                }
                self.next_backward_cursor = i;
            }
            Err(i) => {
                self.next_backward_cursor = i;
            }
        }
    }

    fn advance_forwards(&mut self, len: usize) {
        self.start_idx = self.end_idx;
        self.end_idx += len;

        let mut i = self.next_forward_cursor;
        while i < self.forward_cursors.len() && self.forward_cursors[i].start <= self.end_idx {
            self.active_cursors.push(self.forward_cursors[i]);
            i += 1;
        }
        self.next_forward_cursor = i;
        self.remove_inactive();
    }

    fn advance_backwards(&mut self, len: usize) {
        self.end_idx = self.start_idx;
        self.start_idx = self.start_idx.saturating_sub(len);

        let mut i = self.next_backward_cursor;
        while i < self.backward_cursors.len() && self.start_idx < self.backward_cursors[i].end {
            self.active_cursors.push(self.backward_cursors[i]);
            i += 1;
        }
        self.next_backward_cursor = i;

        self.remove_inactive();
    }

    fn remove_inactive(&mut self) {
        let start_idx = self.start_idx;
        let end_idx = self.end_idx;
        self.active_cursors
            .retain(|c| c.end > start_idx && c.start < end_idx);
    }

    /// Fills the provided buffer with samples from the cursor.
    pub fn advance_and_mix<B: DerefMut<Target = [i16]>>(
        &mut self,
        data: &AudioSnippetsData,
        mut buf: B,
        forwards: bool,
    ) {
        if forwards {
            self.advance_forwards(buf.len());
        } else {
            self.advance_backwards(buf.len());
        }

        for c in &self.active_cursors {
            let buf: &mut [i16] = &mut buf;
            let snip = data.snippet(c.id);
            let multiplier = snip.multiplier;

            let snip_start = self.start_idx.saturating_sub(c.start);
            let snip_end = self.end_idx.saturating_sub(c.start).min(snip.buf.len());
            let buf_offset = c.start.saturating_sub(self.start_idx);

            for (idx, sample) in snip.buf[snip_start..snip_end].iter().enumerate() {
                buf[buf_offset + idx] += (*sample as f32 * multiplier) as i16;
            }
        }
    }

    /// Has this cursor finished producing non-zero samples?
    pub fn is_finished(&self) -> bool {
        self.active_cursors.is_empty() && self.next_forward_cursor == self.forward_cursors.len()
    }
}

impl AudioState {
    /// Initializes the audio and spawns the audio thread. Returns an object that can be used
    /// to control the audio.
    pub fn init() -> AudioState {
        let output_data = Arc::new(Mutex::new(OutputData {
            snips: AudioSnippetsData::default(),
            cursor: Cursor::default(),
            forwards: true,
        }));
        let output_pipeline = create_output_pipeline(Arc::clone(&output_data));
        if let Err(e) = &output_pipeline {
            log::error!(
                "Error initializing audio output, there will be no sound: {}",
                e
            );
        }

        let input_data = Arc::new(Mutex::new(InputData { buf: Vec::new() }));
        let input_pipeline = create_input_pipeline(Arc::clone(&input_data));
        if let Err(e) = &input_pipeline {
            log::error!(
                "Error initializing audio input, there will be no sound: {}",
                e
            );
        }
        AudioState {
            output_data,
            output_pipeline: output_pipeline.ok(),
            input_data,
            input_pipeline: input_pipeline.ok(),
        }
    }

    pub fn seek(&mut self, time: Time, velocity: f64) {
        self.output_data.lock().unwrap().forwards = velocity > 0.0;
        let result = || -> Result<()> {
            if let Some(pipe) = self.output_pipeline.as_ref() {
                if let Some(sink) = pipe.get_by_name("playback-sink") {
                    if velocity > 0.0 {
                        sink.seek(
                            velocity,
                            gst::SeekFlags::FLUSH,
                            gst::SeekType::Set,
                            gst::ClockTime::from_useconds(time.as_micros() as u64),
                            gst::SeekType::Set,
                            gst::ClockTime::none(),
                        )?;
                    } else {
                        // There's a very annoying bug, either here or in gstreamer, in which if we
                        // play with velocity 1.0 and then velocity -1.0, it doesn't actually play.
                        // The workaround for now is just not to use velocity -1.0: it only comes
                        // up when scanning, anyway, so we can just ensure that the scanning speed
                        // is never -1.0.
                        sink.seek(
                            velocity,
                            gst::SeekFlags::FLUSH,
                            gst::SeekType::Set,
                            gst::ClockTime::from_useconds(0),
                            gst::SeekType::Set,
                            gst::ClockTime::from_useconds(time.as_micros() as u64),
                        )?;
                    }
                }
            }
            Ok(())
        }();
        if let Err(e) = result {
            log::error!("failed to seek: {}", e);
        }
    }

    pub fn start_recording(&mut self) {
        self.input_data.lock().unwrap().buf.clear();

        if let Some(pipe) = self.input_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Playing) {
                log::error!("failed to start recording audio: {}", e);
            }
        }
    }

    pub fn stop_recording(&mut self) -> Vec<i16> {
        let mut buf = Vec::new();
        std::mem::swap(&mut self.input_data.lock().unwrap().buf, &mut buf);
        if let Some(pipe) = self.output_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Paused) {
                log::error!("failed to pause recording: {}", e);
            }
        }
        process_audio(buf)
    }

    pub fn start_playing(&mut self, data: AudioSnippetsData, time: Time, velocity: f64) {
        {
            let mut lock = self.output_data.lock().unwrap();
            lock.cursor = Cursor::new(&data, time, SAMPLE_RATE);
            lock.snips = data;
            lock.forwards = velocity > 0.0;
        }
        if let Some(pipe) = self.output_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Playing) {
                log::error!("failed to start playing audio: {}", e);
                return;
            }
        }
        self.seek(time, velocity);
    }

    pub fn stop_playing(&mut self) {
        if let Some(pipe) = self.output_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Paused) {
                log::error!("failed to stop audio: {}", e);
            }
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

fn create_input_pipeline(data: Arc<Mutex<InputData>>) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = gst::ElementFactory::make("autoaudiosrc", Some("record-source"))?;
    let sink = gst::ElementFactory::make("appsink", Some("record-sink"))?;
    pipeline.add_many(&[&src, &sink])?;
    gst::Element::link_many(&[&src, &sink])?;

    let sink = sink
        .dynamic_cast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("bug: couldn't cast sink to an AppSink"))?;
    let audio_info =
        gst_audio::AudioInfo::new(gst_audio::AudioFormat::S16le, SAMPLE_RATE as u32, 1).build()?;
    sink.set_caps(Some(&audio_info.to_caps()?));

    let new_sample = move |sink: &gst_app::AppSink| -> Result<gst::FlowSuccess, gst::FlowError> {
        let sample = match sink.pull_sample() {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to pull sample: {}", e);
                return Err(gst::FlowError::CustomError);
            }
        };

        let buffer = match sample.get_buffer() {
            Some(b) => b,
            None => {
                log::error!("Failed to get sample buffer");
                return Err(gst::FlowError::CustomError);
            }
        };

        let buffer = match buffer.map_readable() {
            Ok(b) => b,
            Err(e) => {
                log::error!("Failed to map buffer as readable: {}", e);
                return Err(gst::FlowError::CustomError);
            }
        };

        let buffer = buffer.as_slice();
        let mut lock = data.lock().unwrap();
        for sample in buffer.chunks(2) {
            lock.buf.push(i16::from_le_bytes([sample[0], sample[1]]));
        }
        Ok(gst::FlowSuccess::Ok)
    };
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::new()
            .new_sample(new_sample)
            .build(),
    );
    Ok(pipeline)
}

/// Creates a gstreamer AppSrc element that mixes our audio and provides it to a gstreamer
/// pipeline.
pub fn create_appsrc(data: Arc<Mutex<OutputData>>, name: &str) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("appsrc", Some(name))?;
    let src = src
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow!("bug: couldn't cast src to an AppSrc"))?;
    let audio_info =
        gst_audio::AudioInfo::new(gst_audio::AudioFormat::S16le, SAMPLE_RATE as u32, 1).build()?;
    src.set_caps(Some(&audio_info.to_caps()?));
    src.set_property_format(gst::Format::Time);
    src.set_stream_type(gst_app::AppStreamType::RandomAccess);

    let need_audio_data_inner =
        move |src: &gst_app::AppSrc, size_hint: u32| -> anyhow::Result<()> {
            let mut lock = data.lock().unwrap();
            if lock.cursor.is_finished() {
                let _ = src.end_of_stream();
                return Ok(());
            }

            // I'm not sure if this is necessary, but there isn't much documentation on `size_hint` in
            // gstreamer, so just to be sure let's make sure it isn't too small.
            let size = size_hint.max(1024);

            // gstreamer buffers seem to only ever hand out [u8], but we prefer to work with
            // [i16]s. Here, we're doing an extra copy to handle endian-ness and avoid unsafe.
            let mut buf = vec![0i16; size as usize / 2];
            let forwards = lock.forwards;
            let data: &mut OutputData = &mut lock;
            data.cursor
                .advance_and_mix(&data.snips, &mut buf[..], forwards);
            let time = Time::from_audio_idx(data.cursor.start_idx as i64, SAMPLE_RATE);

            let mut gst_buffer = gst::Buffer::with_size(size as usize)?;
            {
                let gst_buffer_ref = gst_buffer
                    .get_mut()
                    .ok_or(anyhow!("couldn't get mut buffer"))?;

                gst_buffer_ref.set_pts(gst::ClockTime::from_useconds(time.as_micros() as u64));
                let mut data = gst_buffer_ref.map_writable()?;
                for (idx, bytes) in data.as_mut_slice().chunks_mut(2).enumerate() {
                    bytes.copy_from_slice(&buf[idx].to_le_bytes());
                }
            }
            let _ = src.push_buffer(gst_buffer);
            Ok(())
        };

    let need_audio_data = move |src: &gst_app::AppSrc, size_hint: u32| {
        if let Err(e) = need_audio_data_inner(src, size_hint) {
            log::error!("error synthesizing audio: {}", e);
        }
    };

    // The seek callback doesn't actually do anything. That's because we reset the cursor position
    // in `start_playing` anyway, and that's the only meaningful seek that ever happens.
    let seek = move |_src: &gst_app::AppSrc, _arg: u64| -> bool { true };

    src.set_callbacks(
        gst_app::AppSrcCallbacks::new()
            .need_data(need_audio_data)
            .seek_data(seek)
            .build(),
    );
    Ok(src.upcast::<gst::Element>())
}

fn create_output_pipeline(data: Arc<Mutex<OutputData>>) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = create_appsrc(data, "playback-source")?;
    let scale = gst::ElementFactory::make("scaletempo", Some("playback-scale"))?;
    let resample = gst::ElementFactory::make("audioresample", Some("playback-resample"))?;
    let sink = gst::ElementFactory::make("autoaudiosink", Some("playback-sink"))?;

    pipeline.add_many(&[&src, &scale, &resample, &sink])?;
    gst::Element::link_many(&[&src, &scale, &resample, &sink])?;

    Ok(pipeline)
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
        let mut c = Cursor::new(&snips, time::ZERO, 1);
        let mut out = vec![0; 5];
        c.advance_and_mix(&snips, &mut out[..], true);
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn forward_offset() {
        let snips = snips!(5 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, time::ZERO, 1);
        let mut out = vec![0; 15];
        c.advance_and_mix(&snips, &mut out[..], true);
        assert_eq!(out, vec![0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn backward() {
        let snips = snips!(2 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, Time::from_micros(9 * 1000000), 1);
        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], false);
        assert_eq!(out, vec![0, 0, 1, 2, 3, 4, 5, 0, 0, 0]);
    }

    #[test]
    fn backward_already_finished() {
        let snips = snips!(0 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(&snips, Time::from_micros(0), 1);
        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], false);
        assert_eq!(out, vec![0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            2 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, time::ZERO, 1);
        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], true);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets_backwards() {
        let snips = snips!(
            0 => &[1, 2, 3],
            2 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, Time::from_micros(10 * 1000000), 1);
        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], false);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_overlapping_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            12 => &[1, 2, 3]
        );
        let mut c = Cursor::new(&snips, time::ZERO, 1);
        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], true);
        assert_eq!(out, vec![1, 2, 3, 0, 0, 0, 0, 0, 0, 0]);

        let mut out = vec![0; 10];
        c.advance_and_mix(&snips, &mut out[..], true);
        assert_eq!(out, vec![0, 0, 1, 2, 3, 0, 0, 0, 0, 0]);
    }
}
