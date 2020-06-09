use anyhow::Error;
use druid::kurbo::BezPath;
use druid::{Data, Lens, Point, RenderContext};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use scribl_curves::{
    time, Curve, Effect, Effects, FadeEffect, SegmentStyle, SnippetData, SnippetId, SnippetsData,
    Time,
};

use crate::audio::{AudioSnippetData, AudioSnippetId, AudioSnippetsData, AudioState};
use crate::encode::EncodingStatus;
use crate::save_state::SaveFileData;
use crate::undo::{UndoStack, UndoState};
use crate::widgets::ToggleButtonState;

/// While drawing, this stores one continuous poly-line (from pen-down to
/// pen-up). Because we expect lots of fast changes to this, it uses interior
/// mutability to avoid repeated allocations.
#[derive(Clone, Data, Default)]
pub struct SegmentInProgress {
    #[data(ignore)]
    points: Arc<RefCell<Vec<Point>>>,

    #[data(ignore)]
    times: Arc<RefCell<Vec<Time>>>,

    // Data comparison is done using the number of points, which grows with every modification.
    len: usize,
}

impl SegmentInProgress {
    pub fn add_point(&mut self, p: Point, t: Time) {
        self.points.borrow_mut().push(p);
        self.times.borrow_mut().push(t);
        self.len += 1;
    }

    pub fn render(&self, ctx: &mut impl RenderContext, style: SegmentStyle, time: Time) {
        use druid::piet::{LineCap, LineJoin, StrokeStyle};
        let stroke_style = StrokeStyle {
            line_join: Some(LineJoin::Round),
            line_cap: Some(LineCap::Round),
            ..StrokeStyle::new()
        };

        let ps = self.points.borrow();
        if ps.is_empty() {
            return;
        }
        let mut path = BezPath::new();
        path.move_to(ps[0]);
        for p in &ps[1..] {
            path.line_to(*p);
        }
        let last = *self.times.borrow().last().unwrap();
        // TODO: this is copy-paste from Curve
        let color = if let Some(fade) = style.effects.fade() {
            if time >= last + fade.pause + fade.fade {
                return;
            } else if time >= last + fade.pause {
                let ratio =
                    (time - (last + fade.pause)).as_micros() as f64 / fade.fade.as_micros() as f64;
                style.color.with_alpha(1.0 - ratio)
            } else {
                style.color
            }
        } else {
            style.color
        };
        ctx.stroke_styled(&path, &color, style.thickness, &stroke_style);
    }
}

/// A snippet id, an audio snippet id, or neither.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Data)]
pub enum MaybeSnippetId {
    Draw(SnippetId),
    Audio(AudioSnippetId),
    None,
}

impl MaybeSnippetId {
    pub fn is_none(&self) -> bool {
        matches!(self, MaybeSnippetId::None)
    }

    pub fn as_draw(&self) -> Option<SnippetId> {
        if let MaybeSnippetId::Draw(id) = self {
            Some(*id)
        } else {
            None
        }
    }

    pub fn as_audio(&self) -> Option<AudioSnippetId> {
        if let MaybeSnippetId::Audio(id) = self {
            Some(*id)
        } else {
            None
        }
    }
}

impl From<SnippetId> for MaybeSnippetId {
    fn from(id: SnippetId) -> MaybeSnippetId {
        MaybeSnippetId::Draw(id)
    }
}

impl From<AudioSnippetId> for MaybeSnippetId {
    fn from(id: AudioSnippetId) -> MaybeSnippetId {
        MaybeSnippetId::Audio(id)
    }
}

impl Default for MaybeSnippetId {
    fn default() -> MaybeSnippetId {
        MaybeSnippetId::None
    }
}

#[derive(Clone, Data, Default)]
pub struct InProgressStatus {
    pub encoding: Option<f64>,
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

/// This data contains the state of an editor window.
#[derive(Clone, Data, Lens)]
pub struct EditorState {
    pub new_segment: Option<SegmentInProgress>,
    pub new_curve: Option<Arc<Curve>>,

    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub selected_snippet: MaybeSnippetId,

    pub mark: Option<Time>,

    pub action: CurrentAction,
    pub recording_speed: RecordingSpeed,

    // TODO: there doesn't seem to be a lens(ignore) attribute?
    #[lens(name = "ignore_undo")]
    #[data(ignore)]
    pub undo: Arc<RefCell<UndoStack>>,

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

    /// When true, the "fade out" toggle button is pressed down.
    pub fade_enabled: bool,

    pub pen_size: PenSize,

    pub audio: Arc<RefCell<AudioState>>,

    pub palette: crate::widgets::PaletteData,

    // There are several actions that we do asynchronously. Here, we have the most recent status of
    // these actions.
    pub status: AsyncOpsStatus,

    #[data(ignore)]
    pub save_path: Option<PathBuf>,
}

impl Default for EditorState {
    fn default() -> EditorState {
        EditorState {
            new_segment: None,
            new_curve: None,
            snippets: SnippetsData::default(),
            audio_snippets: AudioSnippetsData::default(),
            selected_snippet: MaybeSnippetId::None,
            mark: None,

            action: CurrentAction::Idle,
            recording_speed: RecordingSpeed::Slow,
            undo: Arc::new(RefCell::new(UndoStack::new(UndoState::default()))),

            time_snapshot: (Instant::now(), time::ZERO),
            time: time::ZERO,
            fade_enabled: false,
            pen_size: PenSize::Medium,
            audio: Arc::new(RefCell::new(AudioState::init())),
            palette: crate::widgets::PaletteData::default(),

            status: AsyncOpsStatus::default(),

            save_path: None,
        }
    }
}

impl EditorState {
    fn selected_effects(&self) -> Effects {
        let mut ret = Effects::default();
        if self.fade_enabled {
            ret.add(Effect::Fade(FadeEffect {
                pause: time::Diff::from_micros(250_000),
                fade: time::Diff::from_micros(250_000),
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
        let logical_time_elapsed = time::Diff::from_micros(
            (wall_micros_elapsed as f64 * self.action.time_factor()) as i64,
        );
        self.time_snapshot.1 + logical_time_elapsed
    }

    // Remembers the current time, for calculating time changes later. This should probably be
    // called every time the action changes (TODO: we could make this less error-prone by
    // centralizing the action changes somewhere)
    fn take_time_snapshot(&mut self) {
        self.time_snapshot = (Instant::now(), self.time);
    }

    pub fn start_recording(&mut self, time_factor: f64) {
        assert!(self.new_curve.is_none());
        assert!(self.new_segment.is_none());
        assert_eq!(self.action, CurrentAction::Idle);

        self.action = CurrentAction::WaitingToRecord(time_factor);
        self.take_time_snapshot();
    }

    /// Puts us into the `WaitingToRecord` state, after first cleaning up any
    /// other states that need to be cleaned up. This is useful for handling
    /// mid-drawing undos.
    pub fn ensure_recording(&mut self) {
        match self.action {
            CurrentAction::Playing => self.stop_playing(),
            CurrentAction::Recording(_) => {
                // We don't want to call stop_recording(), because that will
                // clear out the snippet in progress. But we do need to reset
                // the audio.
                self.audio.borrow_mut().stop_playing();
            }
            CurrentAction::RecordingAudio(_) => {
                let _ = self.stop_recording_audio();
            }
            CurrentAction::Scanning(_) => self.stop_scanning(),
            _ => {}
        }
        self.new_segment = None;
        self.action = CurrentAction::WaitingToRecord(self.recording_speed.factor());
        self.take_time_snapshot();
    }

    pub fn start_actually_recording(&mut self) {
        if let CurrentAction::WaitingToRecord(time_factor) = self.action {
            self.action = CurrentAction::Recording(time_factor);
            self.take_time_snapshot();
            if time_factor > 0.0 {
                self.audio.borrow_mut().start_playing(
                    self.audio_snippets.clone(),
                    self.time,
                    time_factor,
                );
            }
        } else {
            panic!("wasn't waiting to record");
        }
    }

    /// Takes the segment that is currently being drawn and adds it to the snippet in progress.
    pub fn add_segment_to_snippet(&mut self, seg: SegmentInProgress) {
        // TODO(performance): this is quadratic for long snippets with lots of segments, because
        // we clone it every time the pen lifts.
        let mut curve = self
            .new_curve
            .as_ref()
            .map(|c| c.as_ref().clone())
            .unwrap_or(Curve::new());
        curve.append_stroke(
            &seg.points.borrow(),
            &seg.times.borrow(),
            self.cur_style(),
            0.0005,
            std::f64::consts::PI / 4.0,
        );
        self.new_curve = Some(Arc::new(curve));
    }

    /// Stops recording drawing, returning the snippet that we just finished recording (if it was
    /// non-empty).
    pub fn stop_recording(&mut self) -> Option<SnippetData> {
        assert!(
            matches!(self.action, CurrentAction::Recording(_) | CurrentAction::WaitingToRecord(_))
        );

        self.audio.borrow_mut().stop_playing();

        if let Some(seg) = self.new_segment.take() {
            // If there is an unfinished segment, we add it directly to the snippet without going
            // through a command, because we don't need the extra undo state.
            self.add_segment_to_snippet(seg);
        }
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
        self.new_curve
            .take()
            .map(|arc_curve| SnippetData::new(arc_curve.as_ref().clone()))
    }

    pub fn start_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::Playing;
        self.take_time_snapshot();
        self.audio
            .borrow_mut()
            .start_playing(self.audio_snippets.clone(), self.time, 1.0);
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
        self.audio.borrow_mut().stop_playing();
    }

    pub fn start_recording_audio(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::RecordingAudio(self.time);
        self.take_time_snapshot();
        self.audio.borrow_mut().start_recording();
    }

    /// Stops recording audio, returning the audio snippet that we just recorded.
    pub fn stop_recording_audio(&mut self) -> AudioSnippetData {
        if let CurrentAction::RecordingAudio(rec_start) = self.action {
            self.action = CurrentAction::Idle;
            self.take_time_snapshot();
            let buf = self.audio.borrow_mut().stop_recording();
            let mut ret = AudioSnippetData::new(buf, rec_start);

            // By default, we normalize to loudness -24. For some reason (possibly to do with
            // incorrectness in the lufs crate), this seems like a good value for avoiding
            // clipping.
            ret.set_multiplier(-24.0);
            ret
        } else {
            panic!("not recording");
        }
    }

    pub fn scan(&mut self, velocity: f64) {
        match self.action {
            CurrentAction::Scanning(cur_vel) if cur_vel != velocity => {
                self.action = CurrentAction::Scanning(velocity);
                // The audio player doesn't support changing direction midstream, and our UI should
                // never put us in that situation, because they have to lift one arrow key before
                // pressing the other.
                assert_eq!(velocity.signum(), cur_vel.signum());
                self.audio.borrow_mut().seek(self.time, velocity);
            }
            CurrentAction::Idle => {
                self.action = CurrentAction::Scanning(velocity);
                self.audio.borrow_mut().start_playing(
                    self.audio_snippets.clone(),
                    self.time,
                    velocity,
                );
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
                self.audio.borrow_mut().stop_playing();
                self.action = CurrentAction::Idle;
                self.take_time_snapshot();
            }
            _ => panic!("not scanning"),
        }
    }

    /// We're starting to load a saved file, so disable user interaction, playing, etc.
    pub fn set_loading(&mut self) {
        match self.action {
            CurrentAction::Scanning(_) => self.stop_scanning(),
            CurrentAction::Recording(_) | CurrentAction::WaitingToRecord(_) => {
                self.stop_recording();
            }
            CurrentAction::Playing => self.stop_playing(),
            CurrentAction::RecordingAudio(_) => {
                self.stop_recording_audio();
            }
            CurrentAction::Idle => {}
            CurrentAction::Loading => {}
        }
        self.action = CurrentAction::Loading;
    }

    pub fn warp_time_to(&mut self, time: Time) {
        self.time = time;
        self.take_time_snapshot();
    }

    pub fn add_to_cur_snippet(&mut self, p: Point, t: Time) {
        assert!(self.action.is_recording());

        if let Some(ref mut snip) = self.new_segment {
            snip.add_point(p, t);
        } else {
            let mut snip = SegmentInProgress::default();
            snip.add_point(p, t);
            self.new_segment = Some(snip);
        }
    }

    pub fn finish_cur_segment(&mut self) -> Option<SegmentInProgress> {
        assert!(self.action.is_recording());
        self.new_segment.take()
    }

    pub fn cur_style(&self) -> SegmentStyle {
        SegmentStyle {
            color: self.palette.selected_color().clone(),
            thickness: self.pen_size.size_fraction(),
            effects: self.selected_effects(),
        }
    }

    pub fn from_save_file(data: SaveFileData) -> EditorState {
        EditorState {
            snippets: data.snippets,
            audio_snippets: data.audio_snippets,
            ..Default::default()
        }
    }

    pub fn to_save_file(&self) -> SaveFileData {
        SaveFileData {
            version: 0,
            snippets: self.snippets.clone(),
            audio_snippets: self.audio_snippets.clone(),
        }
    }

    fn undo_state(&self) -> UndoState {
        UndoState {
            new_curve: self.new_curve.clone(),
            snippets: self.snippets.clone(),
            audio_snippets: self.audio_snippets.clone(),
            selected_snippet: self.selected_snippet.clone(),
            mark: self.mark,
            time: self.time,
        }
    }

    pub fn push_undo_state(&mut self) {
        self.undo.borrow_mut().push(self.undo_state());
    }

    pub fn push_transient_undo_state(&mut self) {
        self.undo.borrow_mut().push_transient(self.undo_state());
    }

    fn restore_undo_state(&mut self, undo: UndoState) {
        let mid_recording = self.new_curve.is_some();

        self.new_curve = undo.new_curve;
        self.snippets = undo.snippets;
        self.audio_snippets = undo.audio_snippets;
        self.selected_snippet = undo.selected_snippet;
        self.mark = undo.mark;
        self.warp_time_to(undo.time);

        // This is a bit of a special-case hack. If there get to be more of
        // these, it might be worth storing some metadata in the undo state.
        //
        // In case the undo resets us to a mid-recording state, we ensure that
        // the state is waiting-to-record (i.e., recording but paused).
        if mid_recording {
            if let Some(new_curve) = self.new_curve.as_ref() {
                if let Some(&time) = new_curve.times.last() {
                    // This is even more of a special-case hack: the end of the
                    // last-drawn curve is likely to be after undo.time (because
                    // undo.time is the time of the beginning of the frame in
                    // which the last curve was drawn). Set the time to be the
                    // end of the last-drawn curve, otherwise they might try to
                    // draw the next segment before the last one finishes.
                    self.warp_time_to(time);
                }
            }
            self.ensure_recording();
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

    /// We've just received this message; update the `status` field to reflect it.
    pub fn update_status(&mut self, msg: &StatusMsg) {
        match msg {
            StatusMsg::Encoding(s) => match s {
                EncodingStatus::Encoding(x) => {
                    self.status.in_progress.encoding = Some(*x);
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
            },
            StatusMsg::Load(path, save_data) => {
                self.status.in_progress.loading = None;
                self.status.last_finished = match save_data {
                    Ok(_) => Some(FinishedStatus::Loaded {
                        path: path.clone(),
                        time: Instant::now(),
                    }),
                    Err(e) => {
                        log::error!("error loading: '{}'", e);
                        Some(FinishedStatus::Error(e.to_string()))
                    }
                };
            }
            StatusMsg::DoneSaving {
                path,
                result,
                autosave: _autosave,
            } => {
                self.status.in_progress.saving = None;
                self.status.last_finished = match result {
                    Ok(()) => {
                        Some(FinishedStatus::Saved {
                            path: path.clone(),
                            // TODO: time should be when it started saving?
                            time: Instant::now(),
                        })
                    }
                    Err(e) => {
                        log::error!("error saving: '{}'", e);
                        Some(FinishedStatus::Error(e.to_string()))
                    }
                }
            }
        }
    }
}

#[derive(Clone, Copy, Data, Debug, PartialEq)]
pub enum CurrentAction {
    /// They started an animation (e.g. by pressing the "video" button), but
    /// haven't actually started drawing yet. The time is not moving; we're
    /// waiting until they start drawing.
    WaitingToRecord(f64),

    /// They are drawing an animation, while the time is ticking.
    Recording(f64),

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
            WaitingToRecord(_) => ToggledOn,
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
        *self == CurrentAction::Idle
    }

    pub fn is_recording(&self) -> bool {
        matches!(*self, CurrentAction::Recording(_))
    }

    pub fn time_factor(&self) -> f64 {
        use CurrentAction::*;
        match *self {
            Playing => 1.0,
            RecordingAudio(_) => 1.0,
            Recording(x) => x,
            Scanning(x) => x,
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

/// We do various operations asynchronously, so the editor keeps a channel open for getting
/// updates. These are the types of messages that can come on the channel.
pub enum StatusMsg {
    // While we're encoding a file, we get these regular status updates from the encoder.
    Encoding(EncodingStatus),
    // We load files asynchronously; when loading is done, we get one of these messages.
    Load(PathBuf, Result<SaveFileData, Error>),
    // When a file is done saving, we get one of these messages.
    DoneSaving {
        path: PathBuf,
        result: Result<(), Error>,
        autosave: bool,
    },
}

impl From<EncodingStatus> for StatusMsg {
    fn from(e: EncodingStatus) -> StatusMsg {
        StatusMsg::Encoding(e)
    }
}
