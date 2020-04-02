// The audio thread takes care of recording and playback. It has two methods of communication: it
// has an ExtEventSink for submitting commands (basically just recorded buffers) to the application,
// and it has a channel for receiving requests.

use cpal::traits::{EventLoopTrait, HostTrait};
use cpal::{EventLoop, StreamData, UnknownTypeInputBuffer, UnknownTypeOutputBuffer};
use druid::Data;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::time::{self, Time};

pub struct AudioState {
    event_loop: Arc<cpal::EventLoop>,
    input_device: cpal::Device,
    output_device: cpal::Device,
    format: cpal::Format,
    input_data: Arc<Mutex<AudioInput>>,
    output_data: Arc<Mutex<AudioOutput>>,
}

pub const SAMPLE_RATE: u32 = 48000;

#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct AudioSnippetId(u64);

#[derive(Deserialize, Serialize, Clone, Data)]
pub struct AudioSnippetData {
    buf: Arc<Vec<i16>>,
    start_time: Time,
}

#[derive(Deserialize, Serialize, Clone, Data, Default)]
pub struct AudioSnippetsData {
    last_id: u64,
    snippets: Arc<BTreeMap<AudioSnippetId, AudioSnippetData>>,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct BufCursor {
    id: AudioSnippetId,
    idx: i64,
    len: i64,
}

#[derive(Default, Debug)]
pub struct Cursor {
    all_cursors: Vec<BufCursor>,
    next_cursor: usize,
    active_cursors: Vec<BufCursor>,
}

impl BufCursor {
    fn new(id: AudioSnippetId, snip: &AudioSnippetData, time: Time) -> BufCursor {
        BufCursor {
            id,
            idx: (time - snip.start_time).as_audio_idx(SAMPLE_RATE),
            len: snip.buf.len() as i64,
        }
    }

    fn get_buf_and_advance<'a>(
        &mut self,
        data: &'a AudioSnippetsData,
        len: usize,
    ) -> (&'a [i16], usize) {
        debug_assert!(self.idx < self.len);
        debug_assert!(self.idx + (len as i64) > 0);

        let snip = data.snippet(self.id);
        let start = self.idx.max(0) as usize;
        let end = ((self.idx + (len as i64)) as usize).min(snip.buf.len());
        let offset = (-self.idx).max(0) as usize;
        self.idx += len as i64;
        (&snip.buf[start..end], offset)
    }

    fn will_be_active(&self, len: usize) -> bool {
        self.idx + (len as i64) > 0
    }

    fn is_finished(&self) -> bool {
        self.idx >= self.len
    }
}

impl Cursor {
    pub fn new(snippets: &AudioSnippetsData, time: Time) -> Cursor {
        let mut cursors = Vec::new();

        for (&id, snip) in snippets.snippets.iter() {
            cursors.push(BufCursor::new(id, snip, time));
        }
        cursors.sort_by_key(|c| -c.idx);

        let mut active = Vec::new();
        let mut next_cursor = cursors.len();
        for (c_idx, c) in cursors.iter().enumerate() {
            if c.idx < 0 {
                next_cursor = c_idx;
                break;
            }

            let snip = snippets.snippet(c.id);
            if c.idx < snip.buf.len() as i64 {
                active.push(*c);
            }
        }

        Cursor {
            all_cursors: cursors,
            next_cursor,
            active_cursors: active,
        }
    }

    pub fn mix_to_buffer<B: DerefMut<Target = [i16]>>(
        &mut self,
        data: &AudioSnippetsData,
        mut buf: B,
        speed_factor: f64,
    ) {
        assert!(speed_factor > 0.0);

        while self.next_cursor < self.all_cursors.len() {
            if self.all_cursors[self.next_cursor].will_be_active(buf.len()) {
                self.active_cursors.push(self.all_cursors[self.next_cursor]);
                self.next_cursor += 1;
            } else {
                break;
            }
        }

        // How many bytes do we need from the input buffers?
        let input_len = (buf.len() as f64 * speed_factor).ceil() as usize;
        // TODO: we do a lot of rounding here. Maybe we should work with floats internally?
        for c in &mut self.active_cursors {
            let (in_buf, offset) = c.get_buf_and_advance(data, input_len);
            let out_buf = &mut buf[offset..];

            for (out_idx, out_sample) in out_buf.iter_mut().enumerate() {
                let in_idx = out_idx as f64 * speed_factor;
                let in_idx0 = in_idx.floor() as usize;
                let in_idx1 = in_idx.ceil() as usize;
                if in_idx1 >= in_buf.len() {
                    // If this cursor is ending, its buffer will finish before
                    // the output buffer does.
                    break;
                }

                let weight = in_idx.fract();
                let in_sample =
                    in_buf[in_idx0] as f64 * (1.0 - weight) + in_buf[in_idx1] as f64 * weight;
                *out_sample = out_sample.saturating_add(in_sample as i16);
            }
        }
        self.active_cursors.retain(|c| !c.is_finished());
    }

    pub fn is_finished(&self) -> bool {
        self.active_cursors.is_empty() && self.next_cursor == self.all_cursors.len()
    }
}

impl AudioState {
    pub fn init() -> AudioState {
        let host = cpal::default_host();
        let event_loop = host.event_loop();
        let input_device = host.default_input_device().expect("no input device");
        let output_device = host.default_output_device().expect("no input device");
        let format = cpal::Format {
            channels: 1,
            sample_rate: cpal::SampleRate(SAMPLE_RATE as u32),
            data_type: cpal::SampleFormat::I16,
        };

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

    pub fn start_recording(&mut self) {
        let input_stream = self
            .event_loop
            .build_input_stream(&self.input_device, &self.format)
            .expect("couldn't build input stream");

        {
            let mut input = self.input_data.lock().unwrap();
            assert!(input.id.is_none());
            input.id = Some(input_stream.clone());
            input.buf.clear();
        }

        self.event_loop
            .play_stream(input_stream)
            .expect("failed to play");
    }

    pub fn stop_recording(&mut self) -> Vec<i16> {
        let mut input_data = self.input_data.lock().unwrap();
        self.event_loop
            .destroy_stream(input_data.id.take().unwrap());

        let mut buf = Vec::new();
        std::mem::swap(&mut input_data.buf, &mut buf);

        // Denoise the recorded audio. (TODO: we could do this in real-time as it records)
        // RNNoise like the input to be a multiple of FRAME_SIZE;
        let fs = rnnoise_c::FRAME_SIZE;
        let new_size = ((buf.len() + (fs - 1)) / fs) * fs;
        buf.resize(new_size, 0);
        let float_buf: Vec<f32> = buf.iter().map(|x| *x as f32).collect();
        let mut out_buf = vec![0.0f32; float_buf.len()];
        let mut state = rnnoise_c::DenoiseState::new();
        for (in_chunk, out_chunk) in float_buf.chunks_exact(fs).zip(out_buf.chunks_exact_mut(fs)) {
            state.process_frame_mut(in_chunk, out_chunk);
        }
        out_buf.into_iter().map(|x| x as i16).collect()
    }

    pub fn start_playing(&mut self, data: AudioSnippetsData, time: Time, velocity: f64) {
        let cursor = Cursor::new(&data, time);
        let output_stream = self
            .event_loop
            .build_output_stream(&self.output_device, &self.format)
            .expect("couldn't build output stream");

        {
            let mut output = self.output_data.lock().unwrap();
            assert!(output.id.is_none());
            output.id = Some(output_stream.clone());
            output.bufs = data;
            output.speed_factor = velocity;
            output.cursor = cursor;
        }

        self.event_loop
            .play_stream(output_stream)
            .expect("failed to play");
    }

    pub fn stop_playing(&mut self) {
        let mut output = self.output_data.lock().unwrap();
        if output.id.is_some() {
            self.event_loop.destroy_stream(output.id.take().unwrap());
        }
    }
}

impl AudioSnippetData {
    pub fn new(buf: Vec<i16>, start_time: Time) -> AudioSnippetData {
        AudioSnippetData {
            buf: Arc::new(buf),
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
}

impl AudioSnippetsData {
    pub fn with_new_snippet(&self, snip: AudioSnippetData) -> AudioSnippetsData {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = AudioSnippetId(ret.last_id);
        let mut map = ret.snippets.deref().clone();
        map.insert(id, snip);
        ret.snippets = Arc::new(map);
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
    event_loop.run(move |stream_id, stream_data| {
        let stream_data = stream_data.expect("error on input stream");
        match stream_data {
            StreamData::Output {
                buffer: UnknownTypeOutputBuffer::I16(mut buf),
            } => {
                let mut output_data = output.lock().unwrap();
                let output_data = output_data.deref_mut();
                if output_data.id.as_ref() != Some(&stream_id) {
                    return;
                }
                for elem in buf.iter_mut() {
                    *elem = 0;
                }
                output_data
                    .cursor
                    .mix_to_buffer(&output_data.bufs, buf, output_data.speed_factor);
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
