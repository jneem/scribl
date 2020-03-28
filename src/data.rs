use druid::kurbo::PathEl;
use druid::{Color, Data, Lens, Point};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ops::Deref;
use std::path::PathBuf;
use std::sync::Arc;

use crate::audio::{AudioSnippetData, AudioSnippetsData, AudioState};
use crate::lerp::Lerp;
use crate::snippet::{Curve, SnippetId};
use crate::widgets::ToggleButtonState;

#[derive(Clone, Data)]
pub struct CurveInProgressData {
    #[druid(ignore)]
    inner: Arc<RefCell<Curve>>,

    // Data comparison is done using only the curve's length, since the length grows with
    // every modification.
    len: usize,
}

impl CurveInProgressData {
    pub fn new(color: &Color, thickness: f64) -> CurveInProgressData {
        CurveInProgressData {
            inner: Arc::new(RefCell::new(Curve::new(color, thickness))),
            len: 0,
        }
    }

    pub fn move_to(&mut self, p: Point, time: i64) {
        self.inner.borrow_mut().move_to(p, time);
        self.len += 1;
    }

    pub fn line_to(&mut self, p: Point, time: i64) {
        self.inner.borrow_mut().line_to(p, time);
        self.len += 1;
    }

    pub fn into_curve(self) -> Curve {
        self.inner.replace(Curve::new(&Color::rgb8(0, 0, 0), 1.0))
    }
}

#[derive(Deserialize, Serialize, Data, Debug, Clone)]
pub struct SnippetData {
    pub curve: Arc<Curve>,
    pub lerp: Arc<Lerp>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub end: Option<i64>,
}

#[derive(Deserialize, Serialize, Clone, Data, Default)]
pub struct SnippetsData {
    last_id: u64,
    snippets: Arc<BTreeMap<SnippetId, SnippetData>>,
}

impl SnippetData {
    // TODO: this panics if the curve is empty
    pub fn new(curve: Curve) -> SnippetData {
        let start_end = vec![
            *curve.time_us.first().unwrap(),
            *curve.time_us.last().unwrap(),
        ];
        let lerp = Lerp::new(start_end.clone(), start_end);
        SnippetData {
            curve: Arc::new(curve),
            lerp: Arc::new(lerp),
            end: None,
        }
    }

    pub fn path_at(&self, time_us: i64) -> &[PathEl] {
        if let Some(end) = self.end {
            if time_us > end {
                return &[];
            }
        }

        let local_time = self.lerp.unlerp_clamped(time_us);
        let idx = match self.curve.time_us.binary_search(&local_time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.curve.path.elements()[..idx]
    }

    pub fn start_time(&self) -> i64 {
        self.lerp.first()
    }

    /// The last time at which the snippet changed.
    pub fn last_draw_time(&self) -> i64 {
        self.lerp.last()
    }

    /// The time at which this snippet should disappear.
    pub fn end_time(&self) -> Option<i64> {
        self.end
    }
}

impl SnippetsData {
    pub fn with_new_snippet(&self, snip: SnippetData) -> SnippetsData {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = SnippetId(ret.last_id);
        let mut map = ret.snippets.deref().clone();
        map.insert(id, snip);
        ret.snippets = Arc::new(map);
        ret
    }

    pub fn with_replacement_snippet(&self, id: SnippetId, new: SnippetData) -> SnippetsData {
        assert!(id.0 <= self.last_id);
        let mut ret = self.clone();
        let mut map = ret.snippets.deref().clone();
        map.insert(id, new);
        ret.snippets = Arc::new(map);
        ret
    }

    pub fn with_new_lerp(&self, id: SnippetId, lerp_from: i64, lerp_to: i64) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.lerp = Arc::new(snip.lerp.with_new_lerp(lerp_from, lerp_to));
        self.with_replacement_snippet(id, snip)
    }

    pub fn with_truncated_snippet(&self, id: SnippetId, time: i64) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.end = Some(time);
        self.with_replacement_snippet(id, snip)
    }

    pub fn snippet(&self, id: SnippetId) -> &SnippetData {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (SnippetId, &SnippetData)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }
}

#[derive(Deserialize, Serialize)]
pub struct SaveFileData {
    pub version: u64,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
}

/// This data contains the entire state of the app.
#[derive(Clone, Data, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgressData>,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub selected_snippet: Option<SnippetId>,

    pub time_us: i64,
    pub mark: Option<i64>,
}

#[derive(Clone, Data, Lens)]
pub struct AppState {
    pub scribble: ScribbleState,
    pub action: CurrentAction,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
    pub mouse_down: bool,

    pub line_thickness: f64,
    pub line_color: Color,

    pub audio: Arc<RefCell<AudioState>>,

    #[druid(ignore)]
    pub save_path: Option<PathBuf>,
}

impl Default for AppState {
    fn default() -> AppState {
        AppState {
            scribble: ScribbleState::default(),
            action: CurrentAction::Idle,
            mouse_down: false,
            line_thickness: 5.0,
            line_color: Color::rgb8(0, 255, 0),
            audio: Arc::new(RefCell::new(AudioState::init())),

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
            time_us: 0,
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

    pub fn start_recording(&mut self) {
        assert!(self.scribble.new_snippet.is_none());
        assert_eq!(self.action, CurrentAction::Idle);

        self.scribble.new_snippet = Some(CurveInProgressData::new(
            &self.line_color,
            self.line_thickness,
        ));
        self.action = CurrentAction::WaitingToRecord;
    }

    /// Stops recording drawing, returning the snippet that we just finished recording (if it was
    /// non-empty).
    pub fn stop_recording(&mut self) -> Option<SnippetData> {
        assert!(
            self.action == CurrentAction::Recording
                || self.action == CurrentAction::WaitingToRecord
        );
        let new_snippet = self
            .scribble
            .new_snippet
            .take()
            .expect("Tried to stop recording, but we hadn't started!");
        self.action = CurrentAction::Idle;
        let new_curve = new_snippet.into_curve();
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
            .start_playing(self.scribble.audio_snippets.clone(), self.scribble.time_us);
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
        self.audio.borrow_mut().stop_playing();
    }

    pub fn start_recording_audio(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::RecordingAudio(self.scribble.time_us);
        self.audio.borrow_mut().start_recording();
    }

    /// Stops recording audio, returning the audio snippet that we just recorded.
    pub fn stop_recording_audio(&mut self) -> AudioSnippetData {
        self.action = CurrentAction::Idle;
        if let CurrentAction::RecordingAudio(rec_start) = self.action {
            let buf = self.audio.borrow_mut().stop_recording();
            dbg!(buf.len());
            AudioSnippetData::new(buf, rec_start)
        //self.audio_snippets = self.audio_snippets.with_new_snippet(buf, rec_start);
        } else {
            panic!("not recording");
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
    WaitingToRecord,
    Recording,
    Playing,

    /// The argument is the time at which audio capture started.
    RecordingAudio(i64),

    /// Fast-forward or reverse. The parameter is the speed factor, negative for reverse.
    Scanning(f64),
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
            WaitingToRecord => ToggledOn,
            Recording => ToggledOn,
            Idle => ToggledOff,
            Playing => Disabled,
            Scanning(_) => Disabled,
            RecordingAudio(_) => Disabled,
        }
    }

    pub fn play_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            WaitingToRecord => Disabled,
            Recording => Disabled,
            Scanning(_) => Disabled,
            Playing => ToggledOn,
            Idle => ToggledOff,
            RecordingAudio(_) => Disabled,
        }
    }

    pub fn rec_audio_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            WaitingToRecord => Disabled,
            Recording => Disabled,
            Scanning(_) => Disabled,
            Playing => Disabled,
            Idle => ToggledOff,
            RecordingAudio(_) => ToggledOn,
        }
    }

    pub fn is_idle(&self) -> bool {
        *self == CurrentAction::Idle
    }

    pub fn is_recording(&self) -> bool {
        *self == CurrentAction::Recording
    }

    pub fn is_waiting_to_record(&self) -> bool {
        *self == CurrentAction::WaitingToRecord
    }

    pub fn is_ticking(&self) -> bool {
        use CurrentAction::*;
        match *self {
            Recording | Playing | RecordingAudio(_) => true,
            _ => false,
        }
    }

    pub fn is_scanning(&self) -> bool {
        if let CurrentAction::Scanning(_) = *self {
            true
        } else {
            false
        }
    }
}
