//! This module is in charge of audio (both recording and playback).

use anyhow::{anyhow, Result};
use druid::im::OrdMap;
use druid::Data;
use ebur128::EbuR128;
use gst::prelude::*;
use gst_audio::{AudioFormat, AudioInfo};
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use nnnoiseless::DenoiseState;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::ops::DerefMut;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use scribl_curves::{Cursor, Span, Time, TimeDiff};

use crate::config::AudioInput as InputConfig;

/// This is in charge of the audio event loop, and various other things. There should only be one
/// of these alive at any one time, and it is intended to be long-lived (i.e., create it at startup
/// and just keep it around).
pub struct AudioState {
    output_data: OutputData,
    output_tx: Sender<OutputData>,
    // The pipeline will be `None` if there was an error while creating it. In that case, we
    // already printed an error message so we'll just silently (heh) not play any audio.
    output_pipeline: Option<gst::Pipeline>,

    input_tx: Sender<InputConfig>,
    input_config: InputConfig,
    // This is how the audio thread communicates the received audio back to the main thread: it
    // unlocks this mutex and appends its audio to the buffer. This seems to work ok so far, as
    // we're careful to only hold the mutex for as long as we need to copy the data in or out.
    // But the strategy could do with more testing (TODO). E.g., does gstreamer glitch if we block
    // in appsink? Or does it have enough buffers of its own?
    input_data: Arc<Mutex<InputData>>,
    input_pipeline: Option<gst::Pipeline>,
}

/// This data is shared gets sent from the main program to the gstreamer playback pipeline (as
/// represented by the `needs_data` callback on its `app-src` element) whenever the main
/// program wants to update the playback parameters.
#[derive(Clone)]
pub struct OutputData {
    pub snips: AudioSnippetsData,
    pub start_time: Time,
    pub forwards: bool,
}

struct InputData {
    buf: Vec<i16>,
    /// For every frame (of size `DenoiseState::FRAME_SIZE`) in `buf`, we store an estimate of how
    /// likely that frame was to contain speech.
    vad: Vec<f32>,
    loudness: EbuR128,
}

pub const SAMPLE_RATE: u32 = 48000;

// We don't simply drop frames where voice was not detected: doing so tends to cut off consonants
// like "t". Instead, we do some "smoothing in time": if no voice was detected within
// `VOICELESS_FRAME_LAG` frames (either forwards or backwards in time) of the current frame, we
// drop the current frame.
const VOICELESS_FRAME_LAG: usize = 10;

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

impl InputData {
    fn new() -> InputData {
        InputData {
            buf: Vec::new(),
            vad: Vec::new(),
            // TODO: what are the failure cases for Ebur128::new?
            loudness: EbuR128::new(
                1,
                SAMPLE_RATE,
                ebur128::Mode::I | ebur128::Mode::M | ebur128::Mode::SAMPLE_PEAK,
            )
            .unwrap(),
        }
    }

    fn append_buffer(&mut self, buf: &[i16], vad: &[f32]) {
        // FIXME: unwrap
        self.loudness.add_frames_i16(buf).unwrap();
        self.vad.extend_from_slice(vad);
        self.buf.extend_from_slice(buf);
    }
}

impl OutputData {
    fn new() -> OutputData {
        OutputData {
            snips: AudioSnippetsData::default(),
            start_time: Time::ZERO,
            forwards: true,
        }
    }
}

impl AudioState {
    /// Initializes the audio and spawns the audio thread. Returns an object that can be used
    /// to control the audio.
    pub fn init() -> AudioState {
        let (output_tx, output_rx) = channel();
        let output_pipeline = create_output_pipeline(output_rx);
        if let Err(e) = &output_pipeline {
            log::error!(
                "Error initializing audio output, there will be no sound: {}",
                e
            );
        }

        let (input_tx, input_rx) = channel();
        let input_data = Arc::new(Mutex::new(InputData::new()));
        let input_pipeline = create_input_pipeline(Arc::clone(&input_data), input_rx);
        if let Err(e) = &input_pipeline {
            log::error!(
                "Error initializing audio input, there will be no sound: {}",
                e
            );
        }
        AudioState {
            output_data: OutputData::new(),
            output_tx,
            output_pipeline: output_pipeline.ok(),
            input_tx,
            input_config: InputConfig::default(),
            input_data,
            input_pipeline: input_pipeline.ok(),
        }
    }

    pub fn seek(&mut self, time: Time, velocity: f64) {
        self.output_data.forwards = velocity > 0.0;
        self.output_data.start_time = time;
        if self.output_tx.send(self.output_data.clone()).is_err() {
            log::error!("audio thread not present while seeking");
        }
        let result = || -> Result<()> {
            if let Some(pipe) = self.output_pipeline.as_ref() {
                if let Some(sink) = pipe.get_by_name("playback-sink") {
                    // The "scaletempo" gstreamer plugin has some issues with playing backwards. We
                    // avoid them by always playing forwards, but adapting our appsrc to produce
                    // the samples backwards.
                    sink.seek(
                        velocity.abs(),
                        gst::SeekFlags::FLUSH,
                        gst::SeekType::Set,
                        gst::ClockTime::from_useconds(time.as_micros() as u64),
                        gst::SeekType::Set,
                        gst::ClockTime::none(),
                    )?;
                }
            }
            Ok(())
        }();
        if let Err(e) = result {
            log::error!("failed to seek: {}", e);
        }
    }

    pub fn start_recording(&mut self, config: InputConfig) {
        {
            let mut lock = self.input_data.lock().unwrap();
            lock.buf.clear();
            lock.vad.clear();
        }
        if self.input_tx.send(config).is_err() {
            log::error!("audio input thread died, no audio will be recorded");
        }

        if let Some(pipe) = self.input_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Playing) {
                log::error!("failed to start recording audio: {}", e);
            }
        }
    }

    pub fn stop_recording(&mut self, target_loudness: f32) -> (Vec<i16>, f32) {
        let mut data = std::mem::replace(
            self.input_data.lock().unwrap().deref_mut(),
            InputData::new(),
        );
        if let Some(pipe) = self.input_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Paused) {
                log::error!("failed to pause recording: {}", e);
            }
        }

        // Which frames are worth keeping, according to voice detection?
        let vad_threshold = self.input_config.vad_threshold;
        let mut keep: Vec<_> = data.vad.iter().map(|&v| v > vad_threshold).collect();
        convolve_bools(&mut keep[..], VOICELESS_FRAME_LAG);

        // Windows for fading in and out when voice is detected or not.
        let present = vec![1.0; DenoiseState::FRAME_SIZE];
        let absent = vec![0.0; DenoiseState::FRAME_SIZE];
        let fade_out: Vec<_> = (0..DenoiseState::FRAME_SIZE)
            .rev()
            .map(|x| x as f32 / DenoiseState::FRAME_SIZE as f32)
            .collect();
        let fade_in: Vec<_> = (0..DenoiseState::FRAME_SIZE)
            .map(|x| x as f32 / DenoiseState::FRAME_SIZE as f32)
            .collect();

        keep.push(false);
        for (frame, k) in data
            .buf
            .chunks_exact_mut(DenoiseState::FRAME_SIZE)
            .zip(keep.windows(2))
        {
            let window = match (k[0], k[1]) {
                (true, true) => &present,
                (false, false) => &absent,
                (true, false) => &fade_out,
                (false, true) => &fade_in,
            };
            for (x, &w) in frame.iter_mut().zip(window) {
                *x = (*x as f32 * w).round() as i16;
            }
        }

        // By default, we normalize to loudness -20. This is quieter than many sources ask for
        // (e.g. youtube recommends -13 to -15), but going louder tends to introduce clipping.
        // Maybe some sort of dynamic range compression would be appropriate?
        let orig_lufs = data.loudness.loudness_global().unwrap() as f32;
        let peak = data.loudness.sample_peak(0).unwrap() as f32;

        // Multiplying a signal by x has the effect of adding 20 * log_10(x) to the loudness.
        let multiplier = 10.0f32
            .powf((target_loudness - orig_lufs) / 20.0)
            // Truncate the multiplier so that we don't clip. (Also make sure the peak isn't really
            // small, because often the sample is all-zero or close to it.)
            .min(1.0 / peak.max(1.0 / 50.0));

        (data.buf, multiplier)
    }

    pub fn start_playing(&mut self, data: AudioSnippetsData, time: Time, velocity: f64) {
        self.output_data.snips = data;
        self.output_data.forwards = velocity > 0.0;
        if self.output_tx.send(self.output_data.clone()).is_err() {
            log::error!("audio thread not present");
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

/// Given a slice of bools, modifies it so that everything within `width` of a `true` is set to
/// `true`.
fn convolve_bools(xs: &mut [bool], width: usize) {
    let mut count = width;
    let next_count = |x, count| if x { 0 } else { count + 1 };

    for x in &mut xs[..] {
        count = next_count(*x, count);
        if count <= width {
            *x = true;
        }
    }

    count = width;
    for x in xs.iter_mut().rev() {
        count = next_count(*x, count);
        if count <= width {
            *x = true;
        }
    }
}

impl AudioSnippetData {
    pub fn new(buf: Vec<i16>, start_time: Time, multiplier: f32) -> AudioSnippetData {
        AudioSnippetData {
            buf: Arc::new(buf),
            multiplier,
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
        let length = TimeDiff::from_audio_idx(self.buf().len() as i64, SAMPLE_RATE);
        self.start_time() + length
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
            .unwrap_or(Time::ZERO)
    }

    /// Fills the provided buffer with samples from the cursor, and advance the cursor.
    pub fn mix_to<B: DerefMut<Target = [i16]>>(
        &self,
        cursor: &Cursor<usize, AudioSnippetId>,
        mut buf: B,
    ) {
        for sp in cursor.active_spans() {
            let buf: &mut [i16] = &mut buf;
            let snip = self.snippet(sp.id);
            let multiplier = snip.multiplier;

            let (curs_start, curs_end) = cursor.current();
            let snip_start = curs_start.saturating_sub(sp.start);
            let snip_end = curs_end.saturating_sub(sp.start).min(snip.buf.len());
            let buf_offset = sp.start.saturating_sub(curs_start);

            for (idx, sample) in snip.buf[snip_start..snip_end].iter().enumerate() {
                buf[buf_offset + idx] += (*sample as f32 * multiplier) as i16;
            }
        }
    }

    pub fn snippet_spans<'a>(&'a self) -> impl Iterator<Item = Span<usize, AudioSnippetId>> + 'a {
        self.snippets.iter().map(|(&id, snip)| {
            let start = snip.start_time().as_audio_idx(SAMPLE_RATE);
            let end = start + snip.buf.len();
            Span {
                start,
                end: Some(end),
                id,
            }
        })
    }
}

fn create_input_pipeline(
    data: Arc<Mutex<InputData>>,
    config_rx: Receiver<InputConfig>,
) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = gst::ElementFactory::make("autoaudiosrc", Some("record-source"))?;
    let sink = gst::ElementFactory::make("appsink", Some("record-sink"))?;
    pipeline.add_many(&[&src, &sink])?;
    gst::Element::link_many(&[&src, &sink])?;

    let sink = sink
        .dynamic_cast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("bug: couldn't cast sink to an AppSink"))?;
    let audio_info = AudioInfo::builder(AudioFormat::S16le, SAMPLE_RATE as u32, 1).build()?;
    sink.set_caps(Some(&audio_info.to_caps()?));

    let mut denoise_state = DenoiseState::new();
    let mut denoise_in_buf = Vec::with_capacity(DenoiseState::FRAME_SIZE);
    let mut denoise_out_buf = vec![0.0; DenoiseState::FRAME_SIZE];
    let mut i16_buf = Vec::with_capacity(DenoiseState::FRAME_SIZE);
    let mut vad_buf = Vec::new();
    let mut config = InputConfig::default();

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

        // The buffer is in bytes; each sample is two bytes.
        let mut buffer = buffer.as_slice();
        for c in config_rx.try_iter() {
            config = c;
        }

        while !buffer.is_empty() {
            let cap_remaining = DenoiseState::FRAME_SIZE - denoise_in_buf.len();
            let size = (buffer.len() / 2).min(cap_remaining);
            for sample in buffer[..(size * 2)].chunks_exact(2) {
                denoise_in_buf.push(i16::from_le_bytes([sample[0], sample[1]]) as f32);
            }
            buffer = &buffer[(size * 2)..];

            let vad = if config.remove_noise {
                denoise_state.process_frame(&mut denoise_out_buf, &denoise_in_buf)
            } else {
                for (&src, dst) in denoise_in_buf.iter().zip(&mut denoise_out_buf[..]) {
                    *dst = src;
                }
                1.0
            };
            denoise_in_buf.clear();

            vad_buf.push(vad);
            for sample in &denoise_out_buf {
                i16_buf.push(sample.round() as i16);
            }
        }

        data.lock().unwrap().append_buffer(&i16_buf, &vad_buf);
        i16_buf.clear();
        vad_buf.clear();
        Ok(gst::FlowSuccess::Ok)
    };
    sink.set_callbacks(
        gst_app::AppSinkCallbacks::builder()
            .new_sample(new_sample)
            .build(),
    );
    Ok(pipeline)
}

/// Creates a gstreamer AppSrc element that mixes our audio and provides it to a gstreamer
/// pipeline.
pub fn create_appsrc(rx: Receiver<OutputData>, name: &str) -> Result<gst::Element> {
    let src = gst::ElementFactory::make("appsrc", Some(name))?;
    let src = src
        .dynamic_cast::<gst_app::AppSrc>()
        .map_err(|_| anyhow!("bug: couldn't cast src to an AppSrc"))?;
    let audio_info = AudioInfo::builder(AudioFormat::S16le, SAMPLE_RATE as u32, 1).build()?;
    src.set_caps(Some(&audio_info.to_caps()?));
    src.set_property_format(gst::Format::Time);
    src.set_stream_type(gst_app::AppStreamType::RandomAccess);

    let mut data = OutputData::new();
    let mut cursor = Cursor::empty(0);
    let mut need_audio_data_inner =
        move |src: &gst_app::AppSrc, size_hint: u32| -> anyhow::Result<()> {
            for new_data in rx.try_iter() {
                data = new_data;
                let idx = data.start_time.as_audio_idx(SAMPLE_RATE);
                cursor = Cursor::new(data.snips.snippet_spans(), idx, idx);
            }
            if data.forwards && cursor.is_finished() || !data.forwards && cursor.current().1 == 0 {
                let _ = src.end_of_stream();
                return Ok(());
            }

            // I'm not sure if this is necessary, but there isn't much documentation on `size_hint` in
            // gstreamer, so just to be sure let's make sure it isn't too small.
            let size = size_hint.max(1024);

            // gstreamer buffers seem to only ever hand out [u8], but we prefer to work with
            // [i16]s. Here, we're doing an extra copy to handle endian-ness and avoid unsafe.
            let mut buf = vec![0i16; size as usize / 2];
            if data.forwards {
                let prev_end = cursor.current().1;
                cursor.advance_to(prev_end, prev_end + buf.len());
            } else {
                let prev_start = cursor.current().0;
                cursor.advance_to(prev_start.saturating_sub(buf.len()), prev_start);
            }
            data.snips.mix_to(&cursor, &mut buf[..]);
            let time = Time::from_audio_idx(cursor.current().0, SAMPLE_RATE);

            let mut gst_buffer = gst::Buffer::with_size(size as usize)?;
            {
                let gst_buffer_ref = gst_buffer
                    .get_mut()
                    .ok_or(anyhow!("couldn't get mut buffer"))?;

                let time = if data.forwards {
                    time
                } else {
                    data.start_time + (data.start_time - time)
                };
                gst_buffer_ref.set_pts(gst::ClockTime::from_useconds(time.as_micros() as u64));
                let mut gst_buf = gst_buffer_ref.map_writable()?;
                if data.forwards {
                    for (idx, bytes) in gst_buf.as_mut_slice().chunks_mut(2).enumerate() {
                        bytes.copy_from_slice(&buf[idx].to_le_bytes());
                    }
                } else {
                    for (idx, bytes) in gst_buf.as_mut_slice().chunks_mut(2).rev().enumerate() {
                        bytes.copy_from_slice(&buf[idx].to_le_bytes());
                    }
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
        gst_app::AppSrcCallbacks::builder()
            .need_data(need_audio_data)
            .seek_data(seek)
            .build(),
    );
    Ok(src.upcast::<gst::Element>())
}

fn create_output_pipeline(rx: Receiver<OutputData>) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = create_appsrc(rx, "playback-source")?;
    let scale = gst::ElementFactory::make("scaletempo", Some("playback-scale"))?;
    let resample = gst::ElementFactory::make("audioresample", Some("playback-resample"))?;
    let sink = gst::ElementFactory::make("autoaudiosink", Some("playback-sink"))?;

    pipeline.add_many(&[&src, &scale, &resample, &sink])?;
    gst::Element::link_many(&[&src, &scale, &resample, &sink])?;

    Ok(pipeline)
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
                    let time = Time::from_audio_idx($time, SAMPLE_RATE);
                    ret = ret.with_new_snippet(AudioSnippetData::new(buf.to_owned(), time));
                )*

                ret
            }
        }
    }

    #[test]
    fn forward() {
        let snips = snips!(0 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(snips.snippet_spans(), 0, 0);
        let mut out = vec![0; 5];
        c.advance_to(0, 5);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn forward_offset() {
        let snips = snips!(5 => &[1, 2, 3, 4, 5]);
        let c = Cursor::new(snips.snippet_spans(), 0, 15);
        let mut out = vec![0; 15];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![0, 0, 0, 0, 1, 2, 3, 4, 5, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn backward() {
        let snips = snips!(3 => &[1, 2, 3, 4, 5]);
        let mut c = Cursor::new(snips.snippet_spans(), 9, 9);
        let mut out = vec![0; 10];
        c.advance_to(0, 10);
        snips.mix_to(&c, &mut out[..]);
        // 2 leading zeros, not 3, because of rounding in audio/index conversion.
        assert_eq!(out, vec![0, 0, 1, 2, 3, 4, 5, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            3 => &[1, 2, 3]
        );
        let c = Cursor::new(snips.snippet_spans(), 0, 10);
        let mut out = vec![0; 10];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn multiple_snippets_backwards() {
        let snips = snips!(
            0 => &[1, 2, 3],
            3 => &[1, 2, 3]
        );
        let mut c = Cursor::new(snips.snippet_spans(), 10, 20);
        let mut out = vec![0; 10];
        c.advance_to(0, 10);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 4, 2, 3, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn non_overlapping_snippets() {
        let snips = snips!(
            0 => &[1, 2, 3],
            12 => &[1, 2, 3]
        );
        let mut c = Cursor::new(snips.snippet_spans(), 0, 10);
        let mut out = vec![0; 10];
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![1, 2, 3, 0, 0, 0, 0, 0, 0, 0]);

        let mut out = vec![0; 10];
        c.advance_to(10, 20);
        snips.mix_to(&c, &mut out[..]);
        assert_eq!(out, vec![0, 0, 1, 2, 3, 0, 0, 0, 0, 0]);
    }
}
