use anyhow::{anyhow, Result};
use crossbeam_channel::{select, unbounded, Receiver, Sender};
use druid::{ExtEventSink, Target};
use ebur128::EbuR128;
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_audio::{AudioFormat, AudioInfo};
use nnnoiseless::DenoiseState;
use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

use scribl_curves::Time;

use crate::cmd;

use super::{
    create_appsrc, create_gst_elt, AudioRecording, AudioRecordingStatus, InputConfig, OutputData,
    TalkSnippet, SAMPLE_RATE,
};

// We don't simply drop frames where voice was not detected: doing so tends to cut off consonants
// like "t". Instead, we do some "smoothing in time": if no voice was detected within
// `VOICELESS_FRAME_LAG` frames (either forwards or backwards in time) of the current frame, we
// drop the current frame.
const VOICELESS_FRAME_LAG: usize = 10;

/// This contains the audio pipelines and the various channels that are used to communicate with
/// the gstreamer callbacks. Essentially, this is the main state in the audio loop.
struct AudioState {
    // The other end of this lives in the app_src callback. We send new output data along here when
    // we want to update the audio data that's playing.
    output_tx: Sender<OutputData>,
    // The pipeline will be `None` if there was an error while creating it. In that case, we
    // already printed an error message so we'll just silently (heh) not play any audio.
    output_pipeline: Option<gst::Pipeline>,
    // The current output data (i.e., a copy of the last thing we sent along output_tx).
    output_data: OutputData,

    // The receiver of this lives in the app_sink callback. We send input configs to it when we
    // want to change the input settings. We send `None` when we want to stop storing the input
    // audio.
    input_tx: Sender<Option<InputConfig>>,
    // The sender of this lives in the app_sink callback. It regularly sends us messages about
    // things like input levels.
    input_status_rx: Receiver<AudioRecordingStatus>,
    // The current input settings (i.e. a copy of the ones that we most recently sent on input_tx).
    input_config: InputConfig,
    // This is how the audio thread communicates the received audio back to the main thread: it
    // unlocks this mutex and appends its audio to the buffer. This seems to work ok so far, as
    // we're careful to only hold the mutex for as long as we need to copy the data in or out.
    // But the strategy could do with more testing (TODO). E.g., does gstreamer glitch if we block
    // in appsink? Or does it have enough buffers of its own?
    input_data: Arc<Mutex<InputData>>,
}

struct InputData {
    buf: Vec<i16>,
    /// For every frame (of size `DenoiseState::FRAME_SIZE`) in `buf`, we store an estimate of how
    /// likely that frame was to contain speech.
    vad: Vec<f32>,
    loudness: EbuR128,
}

/// These are the commands that can be sent to the audio thread.
pub enum Cmd {
    Play(OutputData),
    StopPlaying,
    Record(InputConfig),
    StopRecording(Time),
    Seek(Time, f64),
}

impl AudioState {
    /// Initializes the audio input and output pipelines.
    fn init() -> AudioState {
        let (output_tx, output_rx) = unbounded();
        let output_pipeline = create_output_pipeline(output_rx);
        if let Err(e) = &output_pipeline {
            log::error!(
                "Error initializing audio output, there will be no sound: {}",
                e
            );
        }

        let (input_tx, input_rx) = unbounded();
        let (status_tx, status_rx) = unbounded();
        let input_data = Arc::new(Mutex::new(InputData::new()));
        let input_pipeline = create_input_pipeline(Arc::clone(&input_data), input_rx, status_tx);
        // We keep the input pipeline running, even if we aren't recording audio. This is because
        // starting and starting the input pipeline tends to lead to "pops" in the recording.
        match input_pipeline {
            Err(e) => {
                log::error!(
                    "Error initializing audio input, there will be no audio recording: {}",
                    e
                )
            }
            Ok(pipe) => {
                if let Err(e) = pipe.set_state(gst::State::Playing) {
                    log::error!("failed to start recording audio: {}", e);
                }
            }
        };

        AudioState {
            output_data: OutputData::new(),
            output_tx,
            output_pipeline: output_pipeline.ok(),
            input_tx,
            input_status_rx: status_rx,
            input_config: InputConfig::default(),
            input_data,
        }
    }

    fn seek(&mut self, time: Time, velocity: f64) {
        self.output_data.velocity = velocity;
        self.output_data.start_time = time;
        let result = || -> Result<()> {
            if let Some(pipe) = self.output_pipeline.as_ref() {
                if let Some(sink) = pipe.by_name("playback-sink") {
                    // The "scaletempo" gstreamer plugin has some issues with playing backwards. We
                    // avoid them by always playing forwards, but adapting our appsrc to produce
                    // the samples backwards.
                    sink.seek(
                        velocity.abs(),
                        gst::SeekFlags::FLUSH,
                        gst::SeekType::Set,
                        Some(gst::ClockTime::from_useconds(time.as_micros() as u64)),
                        gst::SeekType::Set,
                        None,
                    )?;
                }
            }
            Ok(())
        }();
        if let Err(e) = result {
            log::error!(
                "failed to seek (time {}, velocity {}): {}",
                e,
                time.as_micros(),
                velocity
            );
        }
    }

    fn start_recording(&mut self, config: InputConfig) {
        self.input_config = config.clone();
        {
            let mut lock = self.input_data.lock().unwrap();
            lock.buf.clear();
            lock.vad.clear();
        }
        if self.input_tx.send(Some(config)).is_err() {
            log::error!("audio input thread died, no audio will be recorded");
        }
    }

    fn stop_recording(&mut self) -> AudioRecording {
        let mut data = std::mem::replace(
            self.input_data.lock().unwrap().deref_mut(),
            InputData::new(),
        );
        if self.input_tx.send(None).is_err() {
            log::error!("audio input thread died, no audio will be recorded");
        }

        // Which frames are worth keeping, according to voice detection?
        let vad_threshold = self.input_config.vad_threshold;
        let mut keep: Vec<_> = data.vad.iter().map(|&v| v > vad_threshold).collect();
        keep.push(false);
        let mut weights = vec![0.0f32; keep.len()];
        convolve_bools(&keep[..], &mut weights[..], VOICELESS_FRAME_LAG);

        // Windows for fading in and out when voice is detected or not.
        let constant = vec![1.0; DenoiseState::FRAME_SIZE];
        let fade_out: Vec<_> = (0..DenoiseState::FRAME_SIZE)
            .rev()
            .map(|x| x as f32 / DenoiseState::FRAME_SIZE as f32)
            .collect();
        let fade_in: Vec<_> = (0..DenoiseState::FRAME_SIZE)
            .map(|x| x as f32 / DenoiseState::FRAME_SIZE as f32)
            .collect();

        for (frame, w) in data
            .buf
            .chunks_exact_mut(DenoiseState::FRAME_SIZE)
            .zip(weights.windows(2))
        {
            let window = if w[0] < w[1] {
                &fade_in
            } else if w[0] > w[1] {
                &fade_out
            } else {
                &constant
            };
            let lo = w[0].min(w[1]);
            let hi = w[0].max(w[1]);
            for (x, &y) in frame.iter_mut().zip(window) {
                let weight = lo + (lo - hi) * y;
                *x = (*x as f32 * weight).round() as i16;
            }
        }

        // Now that we've changed the data, recalculate the loudness.
        data.loudness.reset();
        if let Err(e) = data.loudness.add_frames_i16(&data.buf[..]) {
            log::error!("failed to calculate loudness: {}", e);
        }
        let loudness = data.loudness.loudness_global().unwrap_or(-f64::INFINITY);
        let peak = data.loudness.sample_peak(0).unwrap_or(-f64::INFINITY);
        AudioRecording {
            buf: data.buf,
            loudness,
            peak,
        }
    }

    fn start_playing(&mut self, data: OutputData) {
        self.output_data = data;
        if self.output_tx.send(self.output_data.clone()).is_err() {
            log::error!("audio thread not present");
        }

        if let Some(pipe) = self.output_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Playing) {
                log::error!("failed to start playing audio: {}", e);
                return;
            }
        }
        self.seek(self.output_data.start_time, self.output_data.velocity);
    }

    fn stop_playing(&mut self) {
        if let Some(pipe) = self.output_pipeline.as_ref() {
            if let Err(e) = pipe.set_state(gst::State::Paused) {
                log::error!("failed to stop audio: {}", e);
            }
        }
    }
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

    fn append_buffer(&mut self, buf: &[i16], vad: &[f32]) -> AudioRecordingStatus {
        // What are the error cases here?
        if let Err(e) = self.loudness.add_frames_i16(buf) {
            log::error!("failed to calculate loudness: {}", e);
        }
        self.vad.extend_from_slice(vad);
        self.buf.extend_from_slice(buf);

        AudioRecordingStatus {
            vad: *self.vad.last().unwrap_or(&0.0),
            loudness: self
                .loudness
                .prev_sample_peak(0)
                .ok()
                .map(|x| (x.log10() * 20.0) as f32)
                .unwrap_or(-f32::INFINITY),
        }
    }
}

/// Given a slice of bools, modifies it so that everything within `width` of a `true` is set to
/// `true`.
fn convolve_bools(xs: &[bool], out: &mut [f32], width: usize) {
    let mut count = width;
    let next_count = |x, count| if x { 0 } else { count + 1 };
    let step = 1.0 / width as f32;

    let mut weight = 0.0f32;
    for (&x, y) in xs.iter().zip(&mut out[..]) {
        count = next_count(x, count);
        if count <= width {
            *y = 1.0;
            weight = 1.0;
        } else {
            weight = (weight - step).max(0.0);
            *y = weight;
        }
    }

    count = width;
    weight = 0.0;
    for (&x, y) in xs.iter().zip(&mut out[..]).rev() {
        count = next_count(x, count);
        if count <= width {
            *y = 1.0;
            weight = 1.0;
        } else {
            weight = (weight - step).max(0.0);
            *y = y.max(weight);
        }
    }
}

/// The main function in this module. Spawn it in a new thread, and it will take care of audio
/// input and output. Send commands to it through the `cmd` channel to make it play, stop, record,
/// and so on. The audio loop will send things back through `sink`, targeted at `target`.
pub fn audio_loop(cmd: Receiver<Cmd>, sink: ExtEventSink, target: Target) {
    let mut state = AudioState::init();

    loop {
        select! {
            recv(cmd) -> msg => {
                use Cmd::*;
                match msg {
                    Ok(Play(data)) => state.start_playing(data),
                    Ok(Seek(time, velocity)) => state.seek(time, velocity),
                    Ok(StopPlaying) => state.stop_playing(),
                    Ok(Record(config)) => {
                        state.start_recording(config);
                    }
                    Ok(StopRecording(time)) => {
                        let rec = state.stop_recording();

                        // By default, we normalize to loudness -20. This is quieter than many
                        // sources ask for (e.g. youtube recommends -13 to -15), but going louder
                        // tends to introduce clipping.  Maybe some sort of dynamic range
                        // compression would be appropriate?
                        let target_loudness = -20.0;

                        // Multiplying a signal by x has the effect of adding 20 * log_10(x) to the
                        // loudness.
                        let multiplier = 10.0f64
                            .powf((target_loudness - rec.loudness) / 20.0)
                            // Truncate the multiplier so that we don't clip. (Also make sure the
                            // peak isn't really small, because often the sample is all-zero or
                            // close to it.)
                            .min(1.0 / rec.peak.max(1.0 / 500.0));

                        let snip = TalkSnippet::new(rec.buf, time, multiplier as f32);
                        if let Some(trimmed) = snip.trimmed() {
                            let cmd = cmd::TalkSnippetCmd { snip: trimmed, orig_start: snip.start_time() };
                            let _ = sink.submit_command(cmd::ADD_TALK_SNIPPET, cmd, target);
                        }
                    }
                    Err(_) => {
                        // Failure to receive here just means that the main program exited.
                        break;
                    }
                }
            }
            recv(state.input_status_rx) -> msg => {
                    let _ = sink.submit_command(cmd::RECORDING_AUDIO_STATUS, msg.unwrap(), target);
            }

        }
    }
}

fn create_input_pipeline(
    data: Arc<Mutex<InputData>>,
    config_rx: Receiver<Option<InputConfig>>,
    status_tx: Sender<AudioRecordingStatus>,
) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = create_gst_elt("autoaudiosrc", "record-source")?;
    let resample = create_gst_elt("audioresample", "record-resample")?;
    let convert = create_gst_elt("audioconvert", "record-convert")?;
    let queue = create_gst_elt("queue", "record-queue")?;
    let sink = create_gst_elt("appsink", "record-sink")?;
    pipeline.add_many(&[&src, &resample, &convert, &queue, &sink])?;
    gst::Element::link_many(&[&src, &resample, &convert, &queue, &sink])?;

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
    let mut config = None;

    let new_sample = move |sink: &gst_app::AppSink| -> Result<gst::FlowSuccess, gst::FlowError> {
        let sample = match sink.pull_sample() {
            Ok(s) => s,
            Err(e) => {
                log::error!("Failed to pull sample: {}", e);
                return Err(gst::FlowError::CustomError);
            }
        };

        for c in config_rx.try_iter() {
            config = c;
        }
        let config = match config.as_ref() {
            Some(c) => c,
            None => {
                return Ok(gst::FlowSuccess::Ok);
            }
        };

        let buffer = match sample.buffer() {
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

        let status = data.lock().unwrap().append_buffer(&i16_buf, &vad_buf);
        let _ = status_tx.send(status);
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

fn create_output_pipeline(rx: Receiver<OutputData>) -> Result<gst::Pipeline> {
    let pipeline = gst::Pipeline::new(None);
    let src = create_appsrc(rx, "playback-source")?;
    let scale = create_gst_elt("scaletempo", "playback-scale")?;
    let resample = create_gst_elt("audioresample", "playback-resample")?;
    let convert = create_gst_elt("audioconvert", "playback-convert")?;
    let queue = create_gst_elt("queue", "playback-queue")?;
    let sink = create_gst_elt("autoaudiosink", "playback-sink")?;

    pipeline.add_many(&[&src, &scale, &resample, &convert, &queue, &sink])?;
    gst::Element::link_many(&[&src, &scale, &resample, &convert, &queue, &sink])?;

    Ok(pipeline)
}
