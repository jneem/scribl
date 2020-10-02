use crossbeam_channel::{unbounded, Sender};
use druid::{ExtEventSink, Target};

use scribl_curves::Time;

use super::thread::{audio_loop, Cmd};
use super::{OutputData, TalkSnippets};
use crate::config::AudioInput as InputConfig;
use crate::editor_state::AudioState as State;

/// This is the main interface to an audio thread. It exposes various functions for playing and
/// recording audio.
#[derive(Clone)]
pub struct AudioHandle {
    // Most of the audio action happens on a separate thread; we use this channel to communicate
    // with it.
    cmd_tx: Sender<Cmd>,
}

impl AudioHandle {
    /// Spins up an audio thread, returning a handle to it.
    ///
    /// TODO: figure out, and describe here, the conditions under which the audio thread shuts
    /// down.
    pub fn initialize_audio(sink: ExtEventSink, target: Target) -> AudioHandle {
        let (tx, rx) = unbounded();
        std::thread::spawn(move || audio_loop(rx, sink, target));
        AudioHandle { cmd_tx: tx }
    }

    /// Changes the state of the audio (e.g. from idle to playing or recording).
    ///
    /// If the old state and the new state are the same, this does nothing (and does it pretty
    /// quickly).
    pub fn update(&mut self, old_state: State, new_state: State) {
        use State::*;

        if old_state == new_state {
            return;
        }

        // A special case if we're keeping playing but just changing the speed.
        if let (
            Playing {
                snips: old_snips, ..
            },
            Playing {
                start_time,
                velocity,
                snips,
            },
        ) = (&old_state, &new_state)
        {
            if snips == old_snips {
                self.seek(*start_time, *velocity);
                return;
            }
        }

        match old_state {
            Playing { .. } => self.stop_playing(),
            Recording { start_time, .. } => self.stop_recording(start_time),
            Idle => {}
        }

        match new_state {
            Playing {
                snips,
                start_time,
                velocity,
            } => self.play(snips, start_time, velocity),
            Recording { config, .. } => self.start_recording(config),
            Idle => {}
        }
    }

    /// Start playing audio.
    fn play(&self, snips: TalkSnippets, start_time: Time, velocity: f64) {
        if let Err(e) = self.cmd_tx.send(Cmd::Play(OutputData {
            snips,
            start_time,
            velocity,
        })) {
            log::error!("audio thread exited unexpectedly: {}", e);
        }
    }

    /// Stop playing audio.
    fn stop_playing(&self) {
        if let Err(e) = self.cmd_tx.send(Cmd::StopPlaying) {
            log::error!("audio thread exited unexpectedly: {}", e);
        }
    }

    /// Start recording audio.
    ///
    /// The event sink `sink` is used for sending periodic notifications back to the main app. When
    /// recording is stopped, it will also be used for sending the audio data back to the main app.
    fn start_recording(&self, config: InputConfig) {
        if let Err(e) = self.cmd_tx.send(Cmd::Record(config)) {
            log::error!("audio thread exited unexpectedly: {}", e);
        }
    }

    /// Stop recording audio.
    ///
    /// The resulting audio buffer will be sent as a `ADD_AUDIO_SNIPPET` command.
    fn stop_recording(&self, start_time: Time) {
        if let Err(e) = self.cmd_tx.send(Cmd::StopRecording(start_time)) {
            log::error!("audio thread exited unexpectedly: {}", e);
        }
    }

    /// Seeks the audio to a new location, and possibly also a different speed.
    fn seek(&self, time: Time, velocity: f64) {
        if let Err(e) = self.cmd_tx.send(Cmd::Seek(time, velocity)) {
            log::error!("audio thread exited unexpectedly: {}", e);
        }
    }
}
