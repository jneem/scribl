use druid::{Data, Lens, Point};
use std::path::PathBuf;
use std::time::Instant;

use scribl_curves::{DrawSnippet, DrawSnippetId, StrokeInProgress, StrokeSeq, Time, TimeDiff};

use crate::audio::{TalkSnippetId, TalkSnippets};
use crate::config::Config;
use crate::data::{DenoiseSetting, ScriblState, Settings};
use crate::encode::EncodingStatus;
use crate::undo::{UndoStack, UndoState};
use crate::SaveFileData;

impl From<DrawSnippetId> for SnippetId {
    fn from(id: DrawSnippetId) -> SnippetId {
        SnippetId::Draw(id)
    }
}

impl From<TalkSnippetId> for SnippetId {
    fn from(id: TalkSnippetId) -> SnippetId {
        SnippetId::Talk(id)
    }
}

#[derive(Clone, Data, Default)]
pub struct InProgressStatus {
    pub encoding: Option<(u64, u64)>,
    #[data(same_fn = "PartialEq::eq")]
    pub saving: Option<PathBuf>,
    #[data(same_fn = "PartialEq::eq")]
    pub loading: Option<PathBuf>,
}

#[derive(Clone, Data)]
pub enum FinishedStatus {
    Saved {
        #[data(same_fn = "PartialEq::eq")]
        path: PathBuf,
        #[data(same_fn = "PartialEq::eq")]
        time: Instant,
    },
    Loaded {
        #[data(same_fn = "PartialEq::eq")]
        path: PathBuf,
        #[data(same_fn = "PartialEq::eq")]
        time: Instant,
    },
    Encoded {
        #[data(same_fn = "PartialEq::eq")]
        path: PathBuf,
        #[data(same_fn = "PartialEq::eq")]
        time: Instant,
    },
    Error(String),
}

// This is not the right thing. we should have something for operations in progress,
// something for finished operations, something for errors.
#[derive(Clone, Data, Default)]
pub struct AsyncOpsStatus {
    pub in_progress: InProgressStatus,
    pub last_finished: Option<FinishedStatus>,
}

#[derive(Clone, Data, Debug)]
pub struct RecordingState {
    pub time_factor: f64,
    pub paused: bool,
    pub straight: bool,
    pub new_stroke: StrokeInProgress,
    pub new_stroke_seq: StrokeSeq,
}

#[derive(Copy, Clone, Data, Debug, Eq, Hash, PartialEq)]
pub enum SnippetId {
    Draw(DrawSnippetId),
    Talk(TalkSnippetId),
}

/// This data contains the state of an editor window.
#[derive(Clone, Data, Lens)]
pub struct EditorState {
    pub scribl: ScriblState,
    pub selected_snippet: Option<SnippetId>,
    pub settings: Settings,

    pub mark: Option<Time>,

    pub action: CurrentAction,

    #[lens(ignore)]
    pub undo: UndoStack,

    /// The current (logical) animation time.
    ///
    /// This isn't public because of some invariants that need to be upheld; use `warp_time_to()`
    /// to modify the current time.
    #[lens(name = "time_lens")]
    time: Time,

    /// Here is how our time-keeping works: whenever something changes the
    /// current "speed" (e.g, starting to scan, draw command, etc.), we store the
    /// current wall clock time and the current logical time. Then on every
    /// frame, we use those stored values to update `time`. This is better than
    /// just incrementing `time` based on the inter-frame time, which is prone to
    /// drift.
    #[data(ignore)]
    time_snapshot: (Instant, Time),

    /// The volume of the current audio input, if we're recording audio. This is on a logarithmic
    /// scale (and 0.0 is very loud).
    pub input_loudness: f64,

    // There are several actions that we do asynchronously. Here, we have the most recent status of
    // these actions.
    pub status: AsyncOpsStatus,

    #[data(ignore)]
    pub save_path: Option<PathBuf>,

    #[data(ignore)]
    pub config: Config,

    /// If the file was saved, this is the saved data. It's used to check whether the file has
    /// changed since last save.
    #[data(ignore)]
    pub saved_data: Option<SaveFileData>,
}

impl EditorState {
    pub fn new(config: Config) -> EditorState {
        let mut ret = EditorState {
            scribl: ScriblState::default(),
            settings: Settings::new(&config),
            selected_snippet: None,
            mark: None,

            action: CurrentAction::Idle,
            undo: UndoStack::new(),

            time_snapshot: (Instant::now(), Time::ZERO),
            time: Time::ZERO,
            input_loudness: -f64::INFINITY,

            status: AsyncOpsStatus::default(),

            save_path: None,
            saved_data: None,
            config,
        };
        ret.saved_data = Some(SaveFileData::from_editor_state(&ret));
        ret
    }

    fn with_undo(&mut self, action_text: &str, f: impl FnOnce(&mut EditorState)) {
        let prev_state = self.undo_state();
        f(self);
        self.push_undo_state(prev_state, action_text);
    }

    fn with_transient_undo(&mut self, action_text: &str, f: impl FnOnce(&mut EditorState)) {
        let prev_state = self.undo_state();
        f(self);
        self.push_transient_undo_state(prev_state, action_text);
    }

    fn with_undo_at(&mut self, action_text: &str, time: Time, f: impl FnOnce(&mut EditorState)) {
        let prev_state = self.undo_state();
        f(self);
        self.push_undo_state(prev_state.with_time(time), action_text);
    }

    pub fn add_draw_snippet(&mut self, snip: DrawSnippet) {
        self.with_undo_at("add drawing", snip.start_time(), |state| {
            state.selected_snippet = Some(state.scribl.add_draw_snippet(snip).into());
        });
    }

    pub fn delete_selected_snippet(&mut self) {
        match self.selected_snippet {
            Some(SnippetId::Draw(id)) => {
                self.with_undo("delete drawing", |state| {
                    state.scribl.delete_draw_snippet(id);
                    state.selected_snippet = None;
                });
            }
            Some(SnippetId::Talk(id)) => self.with_undo("delete audio", |state| {
                state.scribl.delete_talk_snippet(id);
                state.selected_snippet = None;
            }),
            None => {
                log::error!("No snippet id to delete");
            }
        }
    }

    /// Truncates the currently selected snippet at the current time.
    ///
    /// This only has an effect if the current snippet is a drawing.
    pub fn truncate_snippet(&mut self) {
        if let Some(SnippetId::Draw(id)) = self.selected_snippet {
            self.with_undo("truncate drawing", |data| {
                data.scribl.draw = data.scribl.draw.with_truncated_snippet(id, data.time());
            });
        } else {
            log::error!("cannot truncate, nothing selected");
        }
    }

    /// "Time-warps" the selected snippet.
    ///
    /// The image that used to be displayed at the marked time will now be displayed at the current
    /// time instead.
    pub fn warp_snippet(&mut self) {
        if let (Some(mark_time), Some(SnippetId::Draw(id))) = (self.mark, self.selected_snippet) {
            self.with_undo("warp drawing", |data| {
                data.scribl.draw = data.scribl.draw.with_new_lerp(id, data.time(), mark_time);
                data.warp_time_to(mark_time);
            });
        } else if self.mark.is_none() {
            log::error!("cannot warp, no marked time");
        } else {
            log::error!("cannot warp, nothing selected");
        }
    }

    /// Shifts the given snippet in time.
    pub fn shift_snippet(&mut self, id: SnippetId, by: TimeDiff) {
        match id {
            SnippetId::Draw(id) => {
                self.with_undo("time-shift drawing", |data| {
                    data.scribl.draw = data.scribl.draw.with_shifted_snippet(id, by);
                });
            }
            SnippetId::Talk(id) => {
                self.with_undo("time-shift speech", |data| {
                    data.scribl.talk = data.scribl.talk.with_shifted_snippet(id, by);
                });
            }
        }
    }

    /// Silences the currently selected range of audio.
    pub fn silence_audio(&mut self) {
        if let (Some(mark_time), Some(SnippetId::Talk(id))) = (self.mark, self.selected_snippet) {
            self.with_undo("silence speech", |data| {
                data.scribl.talk =
                    data.scribl
                        .talk
                        .with_silenced_snippet(id, mark_time, data.time());
            });
        }
    }

    /// Deletes the selected portion of audio.
    ///
    /// If this snippet has more audio after the deleted portion, it will be "moved back."
    pub fn snip_audio(&mut self) {
        if let (Some(mark_time), Some(SnippetId::Talk(id))) = (self.mark, self.selected_snippet) {
            self.with_undo("snip speech", |data| {
                data.scribl.talk =
                    data.scribl
                        .talk
                        .with_snipped_snippet(id, mark_time, data.time());
                if !data.scribl.talk.has_snippet(id) {
                    data.selected_snippet = None;
                }
            });
        }
    }

    /// Multiplies the volume of the selected audio snippet by the given factor.
    pub fn multiply_volume(&mut self, factor: f64) {
        if let Some(SnippetId::Talk(id)) = self.selected_snippet {
            let text = if factor > 1.0 {
                "increase volume"
            } else {
                "decrease volume"
            };
            self.with_undo(text, |data| {
                data.scribl.talk = data.scribl.talk.with_multiplied_snippet(id, factor);
            });
        }
    }

    /// Sets the timeline mark to the current time.
    pub fn set_mark(&mut self) {
        self.with_undo("set mark", |state| state.mark = Some(state.time()));
    }

    /// Removes the current timeline mark.
    pub fn clear_mark(&mut self) {
        if self.mark.is_some() {
            self.with_undo("clear mark", |state| state.mark = None);
        }
    }

    /// Updates `self.time` according to the current wall clock time.
    pub fn update_time(&mut self) {
        self.time = self.accurate_time();
    }

    /// The current logical time.
    pub fn time(&self) -> Time {
        self.time
    }

    /// Our most accurate estimate for the current time.
    ///
    /// [`time`](AppData::time) returns the time at the last frame. This function checks
    /// the elapsed time since the last frame and interpolates the time based on that.
    pub fn accurate_time(&self) -> Time {
        let wall_micros_elapsed = Instant::now()
            .duration_since(self.time_snapshot.0)
            .as_micros();
        let logical_time_elapsed =
            TimeDiff::from_micros((wall_micros_elapsed as f64 * self.action.time_factor()) as i64);
        self.time_snapshot.1 + logical_time_elapsed
    }

    // Remembers the current time, for calculating time changes later. This should probably be
    // called every time the action changes (TODO: we could make this less error-prone by
    // centralizing the action changes somewhere)
    fn take_time_snapshot(&mut self) {
        self.time_snapshot = (Instant::now(), self.time);
    }

    /// Stops recording drawing, returning the snippet that we just finished recording (if it was
    /// non-empty).
    fn stop_recording(&mut self) -> Option<DrawSnippet> {
        self.finish_stroke();
        let old_action = std::mem::replace(&mut self.action, CurrentAction::Idle);
        self.take_time_snapshot();

        if let CurrentAction::Recording(rec_state) = old_action {
            let seq = rec_state.new_stroke_seq.clone();
            if seq.is_empty() {
                None
            } else {
                Some(DrawSnippet::new(seq))
            }
        } else {
            log::error!("tried to stop recording, but we weren't recording");
            None
        }
    }

    pub fn scan(&mut self, velocity: f64) {
        match self.action {
            CurrentAction::Scanning(_) | CurrentAction::Idle => {
                self.action = CurrentAction::Scanning(velocity);
            }
            _ => {
                log::warn!("not scanning, because I'm busy doing {:?}", self.action);
            }
        }
        self.take_time_snapshot();
    }

    /// We're starting to load a saved file, so disable user interaction, playing, etc.
    pub fn set_loading(&mut self) {
        if let CurrentAction::Recording(_) = self.action {
            // If they're drawing, just discard it.
            self.stop_recording();
        }
        self.action = CurrentAction::Loading;
        self.take_time_snapshot();
    }

    pub fn warp_time_to(&mut self, time: Time) {
        self.time = time;
        self.take_time_snapshot();
    }

    pub fn add_point_to_stroke(&mut self, p: Point, t: Time, straight: bool) {
        let mut unpause = false;
        if let CurrentAction::Recording(rec_state) = &mut self.action {
            if straight != rec_state.straight {
                rec_state.straight = straight;
                dbg!(straight);
                rec_state.new_stroke.set_straight(straight);
            }
            rec_state.new_stroke.add_point(p, t);
            if rec_state.paused {
                rec_state.paused = false;
                unpause = true;
            }
        } else {
            log::error!("tried to add a point, but we weren't recording");
        }
        if unpause {
            // We take a time snapshot when unpausing, because that's when the time starts moving
            // forward.
            self.take_time_snapshot();
        }
    }

    pub fn finish_stroke(&mut self) {
        let prev_state = self.undo_state();
        let style = self.settings.cur_style();
        if let CurrentAction::Recording(rec_state) = &mut self.action {
            let stroke = std::mem::replace(&mut rec_state.new_stroke, StrokeInProgress::default());
            rec_state.straight = false;
            let start_time = stroke.start_time().unwrap_or(Time::ZERO);

            // Note that cloning and appending to a StrokeSeq is cheap, because it uses im::Vector
            // internally.
            let mut seq = rec_state.new_stroke_seq.clone();
            seq.append_stroke(stroke, style, 0.0005, std::f64::consts::PI / 4.0);
            rec_state.new_stroke_seq = seq.clone();

            self.push_transient_undo_state(prev_state.with_time(start_time), "add stroke");
        } else {
            log::error!("tried to finish a stroke, but we weren't recording");
        }
    }

    /// Returns a reference to the stroke sequence that is currently being drawn (that is, all the
    /// parts up until the last time that the pen lifted).
    pub fn new_stroke_seq(&self) -> Option<&StrokeSeq> {
        if let CurrentAction::Recording(rec_state) = &self.action {
            Some(&rec_state.new_stroke_seq)
        } else {
            None
        }
    }

    /// Returns a reference to the stroke that is currently being drawn (that is, all the parts
    /// since the last time the pen went down and hasn't come up).
    pub fn new_stroke(&self) -> Option<&StrokeInProgress> {
        if let CurrentAction::Recording(rec_state) = &self.action {
            Some(&rec_state.new_stroke)
        } else {
            None
        }
    }

    pub fn from_save_file(data: SaveFileData, config: Config) -> EditorState {
        let mut ret = EditorState {
            scribl: ScriblState::from_save_file(&data),
            undo: UndoStack::new(),
            ..EditorState::new(config)
        };
        ret.saved_data = Some(data);
        ret
    }

    pub fn undo_state(&self) -> UndoState {
        UndoState {
            snippets: self.scribl.draw.clone(),
            audio_snippets: self.scribl.talk.clone(),
            selected_snippet: self.selected_snippet.clone(),
            mark: self.mark,
            time: self.time,
            action: self.action.clone(),
        }
    }

    pub fn push_undo_state(&mut self, prev_state: UndoState, description: impl ToString) {
        self.undo
            .push(prev_state, self.undo_state(), description.to_string());
    }

    pub fn push_transient_undo_state(&mut self, prev_state: UndoState, description: impl ToString) {
        self.undo
            .push_transient(prev_state, self.undo_state(), description.to_string());
    }

    fn restore_undo_state(&mut self, undo: UndoState) {
        self.scribl.restore_undo_state(&undo);
        self.selected_snippet = undo.selected_snippet;
        self.mark = undo.mark;
        self.warp_time_to(undo.time);
        self.action = CurrentAction::Idle;

        // This is a bit of a special-case hack. If there get to be more of
        // these, it might be worth storing some metadata in the undo state.
        //
        // In case the undo resets us to a mid-recording state, we ensure that
        // the state is recording but paused.
        if let CurrentAction::Recording(mut rec_state) = undo.action {
            rec_state.paused = true;
            rec_state.new_stroke = StrokeInProgress::default();

            if !rec_state.new_stroke_seq.is_empty() {
                // This is even more of a special-case hack: the end of the last-drawn curve is
                // likely to be after undo.time (because undo.time is the time of the beginning of
                // the frame in which the last curve was drawn). Set the time to be the end of the
                // last-drawn curve, otherwise they might try to draw the next segment before the
                // last one finishes.
                self.warp_time_to(rec_state.new_stroke_seq.last_time());
            }

            self.action = CurrentAction::Recording(rec_state);
        }
    }

    pub fn undo(&mut self) {
        let state = self.undo.undo();
        if let Some(state) = state {
            self.restore_undo_state(state);
        }
    }

    pub fn redo(&mut self) {
        let state = self.undo.redo();
        if let Some(state) = state {
            self.restore_undo_state(state);
        }
    }

    pub fn update_encoding_status(&mut self, enc_status: &EncodingStatus) {
        match enc_status {
            EncodingStatus::Encoding { frame, out_of } => {
                self.status.in_progress.encoding = Some((*frame, *out_of));
            }
            EncodingStatus::Finished(path) => {
                self.status.in_progress.encoding = None;
                self.status.last_finished = Some(FinishedStatus::Encoded {
                    path: path.clone(),
                    time: Instant::now(),
                });
            }
            EncodingStatus::Error(s) => {
                self.status.in_progress.encoding = None;
                self.status.last_finished = Some(FinishedStatus::Error(s.clone()));
            }
        }
    }

    pub fn update_load_status(&mut self, load: &crate::cmd::AsyncLoadResult) {
        self.status.in_progress.loading = None;
        self.status.last_finished = match &load.save_data {
            Ok(_) => Some(FinishedStatus::Loaded {
                path: load.path.clone(),
                time: Instant::now(),
            }),
            Err(e) => {
                log::error!("error loading: '{}'", e);
                Some(FinishedStatus::Error(e.to_string()))
            }
        };
    }

    pub fn update_save_status(&mut self, save: &crate::cmd::AsyncSaveResult) {
        self.status.in_progress.saving = None;
        self.status.last_finished = match &save.error {
            None => {
                self.saved_data = Some(save.data.clone());
                Some(FinishedStatus::Saved {
                    path: save.path.clone(),
                    // TODO: time should be when it started saving?
                    time: Instant::now(),
                })
            }
            Some(e) => {
                log::error!("error saving: '{}'", e);
                Some(FinishedStatus::Error(e.clone()))
            }
        }
    }

    pub fn changed_since_last_save(&self) -> bool {
        let new_save = SaveFileData::from_editor_state(self);
        !self.saved_data.same(&Some(new_save))
    }

    pub fn audio_state(&self) -> AudioState {
        use CurrentAction::*;

        let snips = self.scribl.talk.clone();
        let play = |velocity: f64| AudioState::Playing {
            start_time: self.time_snapshot.1,
            snips,
            velocity,
        };

        let mut config = self.config.audio_input.clone();

        // We allow the UI to override what's in the config file.
        match self.settings.denoise_setting {
            DenoiseSetting::DenoiseOn => {
                config.remove_noise = true;
                config.vad_threshold = 0.0;
            }
            DenoiseSetting::DenoiseOff => {
                config.remove_noise = false;
            }
            DenoiseSetting::Vad => {
                config.remove_noise = true;
            }
        }

        match &self.action {
            Playing => play(1.0),
            Scanning(x) => play(*x),
            Recording(state) if !state.paused => play(state.time_factor),
            RecordingAudio(t) => AudioState::Recording {
                start_time: *t,
                config,
            },
            _ => AudioState::Idle,
        }
    }

    pub fn draw(&mut self) {
        self.finish_action();
        self.with_transient_undo("start drawing", |state| {
            state.action = CurrentAction::Recording(RecordingState {
                time_factor: state.settings.recording_speed.factor(),
                paused: true,
                straight: false,
                new_stroke: StrokeInProgress::default(),
                new_stroke_seq: StrokeSeq::default(),
            });
            state.take_time_snapshot();
        });
    }

    pub fn play(&mut self) {
        self.finish_action();
        self.action = CurrentAction::Playing;
        self.take_time_snapshot();
    }

    pub fn talk(&mut self) {
        self.finish_action();
        self.action = CurrentAction::RecordingAudio(self.time);
        self.take_time_snapshot();
    }

    pub fn finish_action(&mut self) {
        match self.action {
            CurrentAction::Recording(_) => {
                if let Some(new_snippet) = self.stop_recording() {
                    self.add_draw_snippet(new_snippet);
                }
            }
            CurrentAction::RecordingAudio(_) => {
                self.input_loudness = -f64::INFINITY;
            }
            _ => {}
        }
        // Note that the editor widget will see this and is in charge of notifying the audio thread
        // if appropriate (i.e. if it needs to stop recordin audio, or stop playing audio).
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
    }
}

#[derive(Clone, Data, Debug)]
pub enum CurrentAction {
    /// They are drawing an animation, while the time is ticking.
    Recording(RecordingState),

    /// They are watching the animation.
    Playing,

    /// The argument is the time at which audio capture started.
    RecordingAudio(Time),

    /// Fast-forward or reverse. The parameter is the speed factor, negative for reverse.
    Scanning(f64),

    /// They aren't doing anything.
    Idle,

    /// We are still loading the file from disk.
    Loading,

    /// We are waiting for some async task to finish, and when it's done we will exit.
    WaitingToExit,
}

impl Default for CurrentAction {
    fn default() -> CurrentAction {
        CurrentAction::Idle
    }
}

impl CurrentAction {
    pub fn is_playing(&self) -> bool {
        matches!(*self, CurrentAction::Playing)
    }

    pub fn is_recording_audio(&self) -> bool {
        matches!(self, &CurrentAction::RecordingAudio(_))
    }

    pub fn is_idle(&self) -> bool {
        matches!(self, CurrentAction::Idle)
    }

    pub fn is_recording(&self) -> bool {
        matches!(*self, CurrentAction::Recording(_))
    }

    pub fn time_factor(&self) -> f64 {
        use CurrentAction::*;
        match self {
            Playing => 1.0,
            RecordingAudio(_) => 1.0,
            Recording(state) => {
                if state.paused {
                    0.0
                } else {
                    state.time_factor
                }
            }
            Scanning(x) => *x,
            _ => 0.0,
        }
    }

    pub fn is_scanning(&self) -> bool {
        matches!(*self, CurrentAction::Scanning(_))
    }
}

/// The current state of the audio subsystem.
#[derive(Clone, PartialEq)]
pub enum AudioState {
    Idle,
    Playing {
        snips: TalkSnippets,
        start_time: Time,
        velocity: f64,
    },
    Recording {
        start_time: Time,
        config: crate::config::AudioInput,
    },
}
