//! This module is in charge of audio (both recording and playback).

use anyhow::{Context, Result};
use gstreamer as gst;

use scribl_curves::Time;

use crate::config::AudioInput as InputConfig;

mod appsrc;
mod handle;
mod snippets;
mod thread;

pub use appsrc::create_appsrc;
pub use handle::AudioHandle;
pub use snippets::{TalkSnippet, TalkSnippetId, TalkSnippets};

/// We do all of our internal audio processing at 48kHz.
pub const SAMPLE_RATE: u32 = 48000;

/// All the information needed to specify some audio for playback (or encoding).
#[derive(Clone)]
pub struct OutputData {
    /// The collection of audio snippets. They will be mixed into the final audio output.
    pub snips: TalkSnippets,
    /// The time at which to start playing.
    pub start_time: Time,
    /// The velocity at which to play back the audio. (1.0 is normal, forwards, playback)
    pub velocity: f64,
}

/// The result of recording audio: a buffer, and a bit of metadata.
pub struct AudioRecording {
    /// The audio signal.
    pub buf: Vec<i16>,
    /// The perceptual loudness (in dB) of the audio.
    pub loudness: f64,
    /// The peak (as a number in [0.0, 1.0]) of the signal.
    pub peak: f64,
}

/// These status messages are sent periodically from the audio thread to the main thread.
#[derive(Clone)]
pub struct AudioRecordingStatus {
    /// The perceptual loudness (in dB) of some recent chunk of audio input.
    pub loudness: f32,
    /// The estimated probability that the input is speech.
    pub vad: f32,
}

impl OutputData {
    fn new() -> OutputData {
        OutputData {
            snips: TalkSnippets::default(),
            start_time: Time::ZERO,
            velocity: 1.0,
        }
    }

    fn forwards(&self) -> bool {
        self.velocity > 0.0
    }
}

fn create_gst_elt(kind: &str, name: &str) -> Result<gst::Element> {
    gst::ElementFactory::make(kind, Some(name)).with_context(|| {
        format!(
            "tried to create {}, of type {}. You are probably missing a gstreamer plugin",
            name, kind
        )
    })
}
