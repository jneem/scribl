use druid::{Data, Lens, Point};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use scribl_curves::{
    DrawSnippet, DrawSnippetId, DrawSnippets, Effect, Effects, FadeEffect, StrokeInProgress,
    StrokeSeq, StrokeStyle, Time, TimeDiff,
};

use crate::audio::{TalkSnippetId, TalkSnippets};
use crate::config::Config;
use crate::encode::EncodingStatus;
use crate::save_state::SaveFileData;
use crate::undo::{UndoStack, UndoState};
use crate::widgets::ToggleButtonState;

/// How far are they allowed to zoom in?
pub const MAX_ZOOM: f64 = 8.0;

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
    pub new_stroke: StrokeInProgress,
    pub new_stroke_seq: Arc<StrokeSeq>,
}

#[derive(Copy, Clone, Data, Debug, Eq, Hash, PartialEq)]
pub enum SnippetId {
    Draw(DrawSnippetId),
    Talk(TalkSnippetId),
}

/// This data contains the state of an editor window.
#[derive(Clone, Data, Lens)]
pub struct EditorState {
    pub snippets: DrawSnippets,
    pub audio_snippets: TalkSnippets,
    pub selected_snippet: Option<SnippetId>,

    pub mark: Option<Time>,

    pub action: CurrentAction,
    pub recording_speed: RecordingSpeed,

    #[lens(ignore)]
    #[data(ignore)]
    pub undo: Arc<RefCell<UndoStack>>,

    /// The current (logical) animation time.
    ///
    /// This isn't public because of some invariants that need to be upheld; use `warp_time_to()`
    /// to modify the current time.
    #[lens(name = "time_lens")]
    time: Time,

    /// Zoom level of the drawing pane. A zoom of 1.0 gives the best fit of the drawing into the
    /// drawing pane; we only allow zooming in from there.
    ///
    /// This is stored here (rather than just in the drawing pane widget) in order
    /// to support menu entries.
    pub zoom: f64,

    /// Here is how our time-keeping works: whenever something changes the
    /// current "speed" (e.g, starting to scan, draw command, etc.), we store the
    /// current wall clock time and the current logical time. Then on every
    /// frame, we use those stored values to update `time`. This is better than
    /// just incrementing `time` based on the inter-frame time, which is prone to
    /// drift.
    #[data(ignore)]
    time_snapshot: (Instant, Time),

    /// When true, the "fade out" toggle button is pressed down.
    pub fade_enabled: bool,

    /// The current pen size, as selected in the UI.
    pub pen_size: PenSize,

    /// The current denoise setting, as selected in the UI.
    pub denoise_setting: DenoiseSetting,

    pub palette: crate::widgets::PaletteData,

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

impl Default for EditorState {
    fn default() -> EditorState {
        let config = crate::config::load_config();
        let denoise_setting = if !config.audio_input.remove_noise {
            DenoiseSetting::DenoiseOff
        } else if config.audio_input.vad_threshold <= 0.0 {
            DenoiseSetting::DenoiseOn
        } else {
            DenoiseSetting::Vad
        };
        let mut ret = EditorState {
            snippets: DrawSnippets::default(),
            audio_snippets: TalkSnippets::default(),
            selected_snippet: None,
            mark: None,

            action: CurrentAction::Idle,
            recording_speed: RecordingSpeed::Slow,
            undo: Arc::new(RefCell::new(UndoStack::new())),

            time_snapshot: (Instant::now(), Time::ZERO),
            time: Time::ZERO,
            zoom: 1.0,
            fade_enabled: false,
            pen_size: PenSize::Medium,
            denoise_setting,
            palette: crate::widgets::PaletteData::default(),

            status: AsyncOpsStatus::default(),

            save_path: None,
            saved_data: None,
            config,
        };
        ret.saved_data = Some(SaveFileData::from_editor_state(&ret));
        ret
    }
}

impl EditorState {
    fn selected_effects(&self) -> Effects {
        let mut ret = Effects::default();
        if self.fade_enabled {
            ret.add(Effect::Fade(FadeEffect {
                pause: TimeDiff::from_micros(250_000),
                fade: TimeDiff::from_micros(250_000),
            }));
        }
        ret
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

    pub fn start_recording(&mut self, time_factor: f64) {
        assert!(matches!(self.action, CurrentAction::Idle));

        self.action = CurrentAction::Recording(RecordingState {
            time_factor,
            paused: true,
            new_stroke: StrokeInProgress::default(),
            new_stroke_seq: Arc::new(StrokeSeq::default()),
        });
        self.take_time_snapshot();
    }

    /// Stops recording drawing, returning the snippet that we just finished recording (if it was
    /// non-empty).
    pub fn stop_recording(&mut self) -> Option<DrawSnippet> {
        self.finish_stroke();
        let old_action = std::mem::replace(&mut self.action, CurrentAction::Idle);
        self.take_time_snapshot();

        if let CurrentAction::Recording(rec_state) = old_action {
            let seq = rec_state.new_stroke_seq.as_ref().clone();
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

    pub fn start_playing(&mut self) {
        assert!(matches!(self.action, CurrentAction::Idle));
        self.action = CurrentAction::Playing;
        self.take_time_snapshot();
    }

    pub fn stop_playing(&mut self) {
        assert!(matches!(self.action, CurrentAction::Playing));
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
    }

    pub fn start_recording_audio(&mut self) {
        assert!(matches!(self.action, CurrentAction::Idle));
        self.action = CurrentAction::RecordingAudio(self.time);
        self.take_time_snapshot();
        let mut config = self.config.audio_input.clone();

        // We allow the UI to override what's in the config file.
        match self.denoise_setting {
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
    }

    /// Stops recording audio.
    ///
    /// If all goes well, the audio thread will soon send a command containing the newly recorded
    /// audio.
    pub fn stop_recording_audio(&mut self) {
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
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

    pub fn stop_scanning(&mut self) {
        match self.action {
            CurrentAction::Scanning(_) => {
                self.action = CurrentAction::Idle;
                self.take_time_snapshot();
            }
            _ => log::error!("not scanning"),
        }
    }

    /// We're starting to load a saved file, so disable user interaction, playing, etc.
    pub fn set_loading(&mut self) {
        match self.action {
            CurrentAction::Scanning(_) => self.stop_scanning(),
            CurrentAction::Recording(_) => {
                self.stop_recording();
            }
            CurrentAction::Playing => self.stop_playing(),
            CurrentAction::RecordingAudio(_) => {
                self.stop_recording_audio();
            }
            CurrentAction::Idle => {}
            CurrentAction::WaitingToExit => {}
            CurrentAction::Loading => {}
        }
        self.action = CurrentAction::Loading;
    }

    pub fn warp_time_to(&mut self, time: Time) {
        self.time = time;
        self.take_time_snapshot();
    }

    pub fn add_point_to_stroke(&mut self, p: Point, t: Time) {
        let mut unpause = false;
        if let CurrentAction::Recording(rec_state) = &mut self.action {
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
        let style = self.cur_style();
        if let CurrentAction::Recording(rec_state) = &mut self.action {
            let stroke = std::mem::replace(&mut rec_state.new_stroke, StrokeInProgress::default());
            let start_time = stroke.start_time().unwrap_or(Time::ZERO);

            // Note that cloning and appending to a StrokeSeq is cheap, because it uses im::Vector
            // internally.
            let mut seq = rec_state.new_stroke_seq.as_ref().clone();
            seq.append_stroke(stroke, style, 0.0005, std::f64::consts::PI / 4.0);
            rec_state.new_stroke_seq = Arc::new(seq);

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

    pub fn cur_style(&self) -> StrokeStyle {
        StrokeStyle {
            color: self.palette.selected_color().clone(),
            thickness: self.pen_size.size_fraction(),
            effects: self.selected_effects(),
        }
    }

    pub fn from_save_file(data: SaveFileData) -> EditorState {
        let mut ret = EditorState {
            snippets: data.snippets.clone(),
            audio_snippets: data.audio_snippets.clone(),
            undo: Arc::new(RefCell::new(UndoStack::new())),
            ..Default::default()
        };
        ret.saved_data = Some(data);
        ret
    }

    pub fn undo_state(&self) -> UndoState {
        UndoState {
            snippets: self.snippets.clone(),
            audio_snippets: self.audio_snippets.clone(),
            selected_snippet: self.selected_snippet.clone(),
            mark: self.mark,
            time: self.time,
            action: self.action.clone(),
        }
    }

    pub fn push_undo_state(&mut self, prev_state: UndoState, description: impl ToString) {
        self.undo
            .borrow_mut()
            .push(prev_state, self.undo_state(), description.to_string());
    }

    pub fn push_transient_undo_state(&mut self, prev_state: UndoState, description: impl ToString) {
        self.undo.borrow_mut().push_transient(
            prev_state,
            self.undo_state(),
            description.to_string(),
        );
    }

    fn restore_undo_state(&mut self, undo: UndoState) {
        self.snippets = undo.snippets;
        self.audio_snippets = undo.audio_snippets;
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
        let state = self.undo.borrow_mut().undo();
        if let Some(state) = state {
            self.restore_undo_state(state);
        }
    }

    pub fn redo(&mut self) {
        let state = self.undo.borrow_mut().redo();
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

        let snips = self.audio_snippets.clone();
        let play = |velocity: f64| AudioState::Playing {
            start_time: self.time_snapshot.1,
            snips,
            velocity,
        };

        match &self.action {
            Playing => play(1.0),
            Scanning(x) => play(*x),
            Recording(state) if !state.paused => play(state.time_factor),
            RecordingAudio(t) => AudioState::Recording {
                start_time: *t,
                config: self.config.audio_input.clone(),
            },
            _ => AudioState::Idle,
        }
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
    pub fn rec_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            Recording(_) => ToggledOn,
            Idle => ToggledOff,
            _ => Disabled,
        }
    }

    pub fn play_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            Playing => ToggledOn,
            Idle => ToggledOff,
            _ => Disabled,
        }
    }

    pub fn rec_audio_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            RecordingAudio(_) => ToggledOn,
            Idle => ToggledOff,
            _ => Disabled,
        }
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

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum RecordingSpeed {
    Paused,
    Slower,
    Slow,
    Normal,
}

impl RecordingSpeed {
    pub fn factor(&self) -> f64 {
        match self {
            RecordingSpeed::Paused => 0.0,
            RecordingSpeed::Slower => 1.0 / 8.0,
            RecordingSpeed::Slow => 1.0 / 3.0,
            RecordingSpeed::Normal => 1.0,
        }
    }
}

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum PenSize {
    Small,
    Medium,
    Big,
}

impl PenSize {
    /// Returns the diameter of the pen, as a fraction of the width of the drawing.
    pub fn size_fraction(&self) -> f64 {
        match self {
            PenSize::Small => 0.002,
            PenSize::Medium => 0.004,
            PenSize::Big => 0.012,
        }
    }
}

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum DenoiseSetting {
    DenoiseOff,
    DenoiseOn,
    Vad,
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
