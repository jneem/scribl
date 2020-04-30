use druid::kurbo::BezPath;
use druid::{Data, Lens, Point};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use scribble_curves::{
    time, Curve, Effect, Effects, FadeEffect, LineStyle, SegmentData, SnippetData, SnippetId,
    SnippetsData, Time,
};

use crate::audio::{AudioSnippetData, AudioSnippetsData, AudioState};
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

    /// Returns a simplified and smoothed version of this polyline.
    ///
    /// `distance_threshold` controls the simplification: higher values will result in
    /// a curve with fewer points. `angle_threshold` affects the presence of angles in
    /// the returned curve: higher values will result in more smooth parts and fewer
    /// angular parts.
    pub fn to_curve(&self, distance_threshold: f64, angle_threshold: f64) -> (BezPath, Vec<Time>) {
        let points = self.points.borrow();
        let times = self.times.borrow();
        let point_indices = scribble_curves::simplify::simplify(&points, distance_threshold);
        let times: Vec<Time> = point_indices.iter().map(|&i| times[i]).collect();
        let points: Vec<Point> = point_indices.iter().map(|&i| points[i]).collect();
        let path = scribble_curves::smooth::smooth(&points, 0.4, angle_threshold);
        (path, times)
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
    pub new_curve: Option<Arc<Curve>>,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub selected_snippet: Option<SnippetId>,

    pub mark: Option<Time>,
}

/// This data contains the state of the entire app.
#[derive(Clone, Data, Lens)]
pub struct AppState {
    pub scribble: ScribbleState,
    pub new_segment: Option<SegmentInProgress>,
    pub action: CurrentAction,
    pub recording_speed: RecordingSpeed,

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
            new_segment: None,
            action: CurrentAction::Idle,
            recording_speed: RecordingSpeed::Slow,

            time_snapshot: (Instant::now(), time::ZERO),
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
            new_curve: None,
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

    /// Updates `self.time` according to the current wall clock time.
    pub fn update_time(&mut self) {
        let wall_micros_elapsed = Instant::now()
            .duration_since(self.time_snapshot.0)
            .as_micros();
        let logical_time_elapsed = time::Diff::from_micros(
            (wall_micros_elapsed as f64 * self.action.time_factor()) as i64,
        );
        self.time = self.time_snapshot.1 + logical_time_elapsed;
    }

    /// The current logical time.
    pub fn time(&self) -> Time {
        self.time
    }

    // Remembers the current time, for calculating time changes later. This should probably be
    // called every time the action changes (TODO: we could make this less error-prone by
    // centralizing the action changes somewhere)
    fn take_time_snapshot(&mut self) {
        self.time_snapshot = (Instant::now(), self.time);
    }

    pub fn start_recording(&mut self, time_factor: f64) {
        assert!(self.scribble.new_curve.is_none());
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
                    self.scribble.audio_snippets.clone(),
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
        let effects = self.selected_effects();
        let style = LineStyle {
            color: self.palette.selected_color().clone(),
            thickness: self.line_thickness,
        };
        let seg_data = SegmentData { effects, style };
        let (path, times) = seg.to_curve(1.0, std::f64::consts::PI / 4.0);
        if let Some(curve) = self.scribble.new_curve.as_ref() {
            let mut curve_clone = curve.as_ref().clone();
            curve_clone.append_segment(path, times, seg_data);
            self.scribble.new_curve = Some(Arc::new(curve_clone));
        } else {
            let mut curve = Curve::new();
            curve.append_segment(path, times, seg_data);
            self.scribble.new_curve = Some(Arc::new(curve));
        }
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
        self.scribble
            .new_curve
            .take()
            .map(|arc_curve| SnippetData::new(arc_curve.as_ref().clone()))
    }

    pub fn start_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::Playing;
        self.take_time_snapshot();
        self.audio
            .borrow_mut()
            .start_playing(self.scribble.audio_snippets.clone(), self.time, 1.0);
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
        self.take_time_snapshot();
        self.audio.borrow_mut().stop_playing();
        dbg!("stopped playing");
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

    /// Returns the new snippet being drawn, converted to a [`Curve`] for your rendering convenience.
    pub fn new_snippet_as_curve(&self) -> Option<Curve> {
        if let Some(ref new_snippet) = self.new_segment {
            let mut ret = Curve::new();
            for (i, (p, t)) in new_snippet
                .points
                .borrow()
                .iter()
                .zip(new_snippet.times.borrow().iter())
                .enumerate()
            {
                if i == 0 {
                    let style = LineStyle {
                        color: self.palette.selected_color().clone(),
                        thickness: self.line_thickness,
                    };
                    let effects = self.selected_effects();
                    ret.move_to(*p, *t, style, effects);
                } else {
                    ret.line_to(*p, *t);
                }
            }
            Some(ret)
        } else {
            None
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
