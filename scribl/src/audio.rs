//! This module is in charge of audio (both recording and playback).

use anyhow::{anyhow, Result};
use druid::im::OrdMap;
use druid::Data;
use gst::prelude::*;
use gstreamer as gst;
use gstreamer_app as gst_app;
use gstreamer_audio as gst_audio;
use nnnoiseless::DenoiseState;
use serde::de::Deserializer;
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

use scribl_curves::{Cursor, Span, Time, TimeDiff};

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
    pub cursor: Cursor<usize, AudioSnippetId>,
    pub forwards: bool,
}

struct InputData {
    buf: Vec<i16>,
}

pub const SAMPLE_RATE: u32 = 48000;
// We truncate the beginning and end of the audio, to avoid capturing the sound of them
// clicking the "record" button.
const TRUNCATION_LEN: TimeDiff = TimeDiff::from_micros(100_000);

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

impl AudioState {
    /// Initializes the audio and spawns the audio thread. Returns an object that can be used
    /// to control the audio.
    pub fn init() -> AudioState {
        let output_data = Arc::new(Mutex::new(OutputData {
            snips: AudioSnippetsData::default(),
            cursor: Cursor::empty(0),
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
        let trunc = buf
            .len()
            .saturating_sub(TRUNCATION_LEN.as_audio_idx(SAMPLE_RATE) as usize);
        buf.truncate(trunc);
        buf
    }

    pub fn start_playing(&mut self, data: AudioSnippetsData, time: Time, velocity: f64) {
        {
            let mut lock = self.output_data.lock().unwrap();
            let idx = time.as_audio_idx(SAMPLE_RATE);
            lock.cursor = Cursor::new(data.snippet_spans(), idx, idx);
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
        let length = TimeDiff::from_audio_idx(self.buf().len() as i64, SAMPLE_RATE);
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

    let mut truncation_remaining: usize = TRUNCATION_LEN.as_audio_idx(SAMPLE_RATE) as usize;
    let mut denoise_state = DenoiseState::new();
    let mut denoise_in_buf = Vec::with_capacity(DenoiseState::FRAME_SIZE);
    let mut denoise_out_buf = vec![0.0; DenoiseState::FRAME_SIZE];
    let mut i16_buf = Vec::with_capacity(DenoiseState::FRAME_SIZE);

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
        if truncation_remaining > 0 {
            let trunc = truncation_remaining.min(buffer.len() / 2);
            truncation_remaining -= trunc;
            buffer = &buffer[(2 * trunc)..];
        }

        while !buffer.is_empty() {
            let cap_remaining = DenoiseState::FRAME_SIZE - denoise_in_buf.len();
            let size = (buffer.len() / 2).min(cap_remaining);
            for sample in buffer[..(size * 2)].chunks_exact(2) {
                denoise_in_buf.push(i16::from_le_bytes([sample[0], sample[1]]) as f32);
            }
            buffer = &buffer[(size * 2)..];

            denoise_state.process_frame(&mut denoise_out_buf, &denoise_in_buf);
            denoise_in_buf.clear();

            for &sample in &denoise_out_buf {
                i16_buf.push(sample as i16);
            }
        }

        {
            let mut lock = data.lock().unwrap();
            lock.buf.extend_from_slice(&i16_buf);
        }
        i16_buf.clear();
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
            if lock.forwards && lock.cursor.is_finished() {
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
            if forwards {
                let prev_end = data.cursor.current().1;
                data.cursor.advance_to(prev_end, prev_end + buf.len());
            } else {
                let prev_start = data.cursor.current().0;
                data.cursor
                    .advance_to(prev_start.saturating_sub(buf.len()), prev_start);
            }
            data.snips.mix_to(&data.cursor, &mut buf[..]);
            let time = Time::from_audio_idx(data.cursor.current().0, SAMPLE_RATE);

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
