use druid::{Color, Data, Lens, Point};
use std::cell::RefCell;
use std::sync::Arc;

use crate::snippet::{Curve, CurveInProgress, LerpedCurve, Snippets, SnippetId};
use crate::widgets::ToggleButtonState;

#[derive(Clone, Data)]
pub struct CurveInProgressData {
    #[druid(ignore)]
    inner: Arc<RefCell<CurveInProgress>>,

    // Data comparison is done using only the curve's length, since the length grows with
    // every modification.
    len: usize,
}

impl CurveInProgressData {
    pub fn new(color: &Color, thickness: f64) -> CurveInProgressData {
        CurveInProgressData {
            inner: Arc::new(RefCell::new(CurveInProgress::new(color, thickness))),
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
        // The cloning isn't so efficient, but replacing the CurveInProgress with a new one is
        // even less efficient because it involves a syscall to get the current time.
        self.inner.borrow().curve.clone()
    }
}

#[derive(Clone, Data, Default)]
pub struct SnippetsData {
    #[druid(ignore)]
    inner: Arc<RefCell<Snippets>>,

    // Increments on every change.
    dirty: u64,
}

impl SnippetsData {
    pub fn insert(&mut self, curve: Curve) -> SnippetId {
        self.dirty += 1;
        self.inner.borrow_mut().insert(curve)
    }

    pub fn snippets(&self) -> std::cell::Ref<Snippets> {
        self.inner.borrow()
    }

    pub fn snippet(&self, id: SnippetId) -> std::cell::Ref<LerpedCurve> {
        std::cell::Ref::map(self.inner.borrow(), |s| &s.curves[&id])
    }

    pub fn snippet_mut(&mut self, id: SnippetId) -> std::cell::RefMut<LerpedCurve> {
        std::cell::RefMut::map(self.inner.borrow_mut(), |s| s.curves.get_mut(&id).unwrap())
    }
}

/// This data contains the entire state of the app.
#[derive(Clone, Data, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgressData>,
    pub snippets: SnippetsData,
    pub selected_snippet: Option<SnippetId>,
    pub action: CurrentAction,

    pub time_us: i64,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
    pub mouse_down: bool,

    pub line_thickness: f64,
    pub line_color: Color,
}

impl Default for ScribbleState {
    fn default() -> ScribbleState {
        ScribbleState {
            new_snippet: None,
            snippets: SnippetsData::default(),
            selected_snippet: None,
            action: CurrentAction::Idle,
            time_us: 0,
            mouse_down: false,
            line_thickness: 5.0,
            line_color: Color::rgb8(0, 255, 0),
        }
    }
}

impl ScribbleState {
    pub fn curve_in_progress<'a>(&'a self) -> Option<impl std::ops::Deref<Target=Curve> + 'a> {
        use std::cell::Ref;
        self.new_snippet.as_ref().map(|s| Ref::map(s.inner.borrow(), |cip| &cip.curve))
    }

    pub fn start_recording(&mut self) {
        assert!(self.new_snippet.is_none());
        assert_eq!(self.action, CurrentAction::Idle);
        dbg!(self.time_us);
        self.new_snippet = Some(CurveInProgressData::new(
            &self.line_color,
            self.line_thickness,
        ));
        self.action = CurrentAction::WaitingToRecord;
    }

    pub fn stop_recording(&mut self) {
        assert!(self.action == CurrentAction::Recording || self.action == CurrentAction::WaitingToRecord);
        let new_snippet = self
            .new_snippet
            .take()
            .expect("Tried to stop recording, but we hadn't started!");
        self.action = CurrentAction::Idle;
        let new_curve = new_snippet.into_curve();
        if !new_curve.path.elements().is_empty() {
            self.snippets.insert(new_curve);
        }
    }

    pub fn start_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::Playing;
        self.time_us = 0;
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
    }
}

#[derive(Clone, Copy, Data, Debug, PartialEq)]
pub enum CurrentAction {
    WaitingToRecord,
    Recording,
    Playing,
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
            Playing => Disabled,
            Idle => ToggledOff,
        }
    }

    pub fn play_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            WaitingToRecord => Disabled,
            Recording => Disabled,
            Playing => ToggledOn,
            Idle => ToggledOff,
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
        *self == CurrentAction::Recording || *self == CurrentAction::Playing
    }
}
