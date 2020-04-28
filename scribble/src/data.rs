use druid::{Color, Data, Lens, Point};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;

use scribble_curves::{
    time, Curve, Effect, Effects, FadeEffect, LineStyle, SnippetData, SnippetId, SnippetsData, Time,
};

use crate::audio::{AudioSnippetData, AudioSnippetsData, AudioState};
use crate::widgets::ToggleButtonState;

#[derive(Clone, Data)]
pub struct CurveInProgressData {
    #[data(ignore)]
    inner: Arc<RefCell<Curve>>,

    #[data(ignore)]
    cur_style: LineStyle,

    #[data(ignore)]
    cur_effects: Effects,

    // Data comparison is done using only the curve's length, since the length grows with
    // every modification.
    len: usize,
}

impl CurveInProgressData {
    pub fn new(color: Color, thickness: f64, effects: Effects) -> CurveInProgressData {
        CurveInProgressData {
            inner: Arc::new(RefCell::new(Curve::new())),
            cur_style: LineStyle {
                color: color,
                thickness,
            },
            cur_effects: effects,
            len: 0,
        }
    }

    pub fn move_to(&mut self, p: Point, time: Time) {
        self.inner
            .borrow_mut()
            .move_to(p, time, self.cur_style.clone(), self.cur_effects.clone());
        self.len += 1;
    }

    pub fn set_color(&mut self, c: Color) {
        self.cur_style.color = c;
    }

    // TODO: note that this isn't actually used yet, because we aren't supporting changing effects in
    // the middle of recording. We support color-changing by using a command, but it's unwieldy to do *every*
    // state change with a command...
    pub fn set_effects(&mut self, effects: Effects) {
        self.cur_effects = effects;
    }

    pub fn line_to(&mut self, p: Point, time: Time) {
        self.inner.borrow_mut().line_to(p, time);
        self.len += 1;
    }

    // TODO: we don't need to consume self, so we could reuse the old curve's memory
    pub fn into_curve(self, distance_threshold: f64, angle_threshold: f64) -> Curve {
        self.inner
            .borrow()
            .smoothed(distance_threshold, angle_threshold)
    }
}

#[derive(Deserialize, Serialize)]
pub struct SaveFileData {
    pub version: u64,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
}

/// This data contains the state of the drawing.
#[derive(Clone, Data, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgressData>,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub selected_snippet: Option<SnippetId>,

    pub mark: Option<Time>,
}

/// This data contains the state of the entire app.
#[derive(Clone, Data, Lens)]
pub struct AppState {
    pub scribble: ScribbleState,
    pub action: CurrentAction,
    pub recording_speed: RecordingSpeed,
    pub time: Time,

    /// When true, the "fade out" toggle button is pressed down.
    pub fade_enabled: bool,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
    pub mouse_down: bool,

    pub line_thickness: f64,

    pub audio: Arc<RefCell<AudioState>>,

    pub palette: crate::widgets::PaletteData,

    pub encoding_status: Option<crate::encode::EncodingStatus>,

    #[data(ignore)]
    pub save_path: Option<PathBuf>,
}

impl Default for AppState {
    fn default() -> AppState {
        AppState {
            scribble: ScribbleState::default(),
            action: CurrentAction::Idle,
            recording_speed: RecordingSpeed::Slow,
            time: time::ZERO,
            fade_enabled: false,
            mouse_down: false,
            line_thickness: 5.0,
            audio: Arc::new(RefCell::new(AudioState::init())),
            palette: crate::widgets::PaletteData::default(),
            encoding_status: None,

            save_path: None,
        }
    }
}

impl Default for ScribbleState {
    fn default() -> ScribbleState {
        ScribbleState {
            new_snippet: None,
            snippets: SnippetsData::default(),
            audio_snippets: AudioSnippetsData::default(),
            selected_snippet: None,
            mark: None,
        }
    }
}

impl AppState {
    pub fn from_save_file(data: SaveFileData) -> AppState {
        AppState {
            scribble: ScribbleState::from_save_file(data),
            ..Default::default()
        }
    }

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

    pub fn start_recording(&mut self, time_factor: f64) {
        assert!(self.scribble.new_snippet.is_none());
        assert_eq!(self.action, CurrentAction::Idle);

        dbg!(self.selected_effects());

        self.scribble.new_snippet = Some(CurveInProgressData::new(
            self.palette.selected_color().clone(),
            self.line_thickness,
            self.selected_effects(),
        ));
        if time_factor > 0.0 {
            self.action = CurrentAction::WaitingToRecord(time_factor);
        } else {
            self.action = CurrentAction::Recording(0.0);
        }
    }

    pub fn start_actually_recording(&mut self) {
        if let CurrentAction::WaitingToRecord(time_factor) = self.action {
            self.action = CurrentAction::Recording(time_factor);
            self.audio.borrow_mut().start_playing(
                self.scribble.audio_snippets.clone(),
                self.time,
                time_factor,
            );
        } else {
            panic!("wasn't waiting to record");
        }
    }

    /// Stops recording drawing, returning the snippet that we just finished recording (if it was
    /// non-empty).
    pub fn stop_recording(&mut self) -> Option<SnippetData> {
        assert!(
            matches!(self.action, CurrentAction::Recording(_) | CurrentAction::WaitingToRecord(_))
        );

        self.audio.borrow_mut().stop_playing();

        let new_snippet = self
            .scribble
            .new_snippet
            .take()
            .expect("Tried to stop recording, but we hadn't started!");
        self.action = CurrentAction::Idle;
        let new_curve = new_snippet.into_curve(1.0, std::f64::consts::PI / 4.0);
        if !new_curve.path.elements().is_empty() {
            Some(SnippetData::new(new_curve))
        } else {
            None
        }
    }

    pub fn start_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::Playing;
        self.audio
            .borrow_mut()
            .start_playing(self.scribble.audio_snippets.clone(), self.time, 1.0);
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
        self.audio.borrow_mut().stop_playing();
    }

    pub fn start_recording_audio(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::RecordingAudio(self.time);
        self.audio.borrow_mut().start_recording();
    }

    /// Stops recording audio, returning the audio snippet that we just recorded.
    pub fn stop_recording_audio(&mut self) -> AudioSnippetData {
        if let CurrentAction::RecordingAudio(rec_start) = self.action {
            self.action = CurrentAction::Idle;
            let buf = self.audio.borrow_mut().stop_recording();
            AudioSnippetData::new(buf, rec_start)
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
                self.audio.borrow_mut().set_velocity(velocity);
            }
            CurrentAction::Idle => {
                self.action = CurrentAction::Scanning(velocity);
                self.audio.borrow_mut().start_playing(
                    self.scribble.audio_snippets.clone(),
                    self.time,
                    velocity,
                );
            }
            _ => {
                log::warn!("not scanning, because I'm busy doing {:?}", self.action);
            }
        }
    }

    pub fn stop_scanning(&mut self) {
        match self.action {
            CurrentAction::Scanning(_) => {
                self.audio.borrow_mut().stop_playing();
                self.action = CurrentAction::Idle;
            }
            _ => panic!("not scanning"),
        }
    }
}

impl ScribbleState {
    pub fn from_save_file(data: SaveFileData) -> ScribbleState {
        ScribbleState {
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

    pub fn curve_in_progress<'a>(&'a self) -> Option<impl std::ops::Deref<Target = Curve> + 'a> {
        self.new_snippet.as_ref().map(|s| s.inner.borrow())
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
            Recording(x) if x > 0.0 => ToggledOn,
            Idle => ToggledOff,
            _ => Disabled,
        }
    }

    pub fn snapshot_toggle(&self) -> ToggleButtonState {
        match *self {
            CurrentAction::Recording(x) if x == 0.0 => ToggleButtonState::ToggledOn,
            CurrentAction::Idle => ToggleButtonState::ToggledOff,
            _ => ToggleButtonState::Disabled,
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

    pub fn is_waiting_to_record(&self) -> bool {
        matches!(*self, CurrentAction::WaitingToRecord(_))
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
    Slower,
    Slow,
    Normal,
}

impl RecordingSpeed {
    pub fn factor(&self) -> f64 {
        match self {
            RecordingSpeed::Slower => 1.0 / 8.0,
            RecordingSpeed::Slow => 1.0 / 3.0,
            RecordingSpeed::Normal => 1.0,
        }
    }
}
