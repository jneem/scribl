use druid::{Data, Lens, Point, TimerToken};
use std::cell::RefCell;
use std::convert::TryInto;
use std::sync::Arc;
use std::time::Instant;

use crate::snippet::{Curve, Snippets};
use crate::widgets::ToggleButtonState;

/// A curve that is currently being drawn.
#[derive(Clone, Data, Lens)]
pub struct CurveInProgress {
    curve: Arc<RefCell<Curve>>,

    /// The time (in microseconds) that this curve started. This is in logical time (i.e., relative
    /// to the animation being created).
    logical_start_time_us: i64,

    /// The actual time (according to the system time) that we started recording this curve.
    #[druid(ignore)]
    wall_start_time: Instant,
}

impl CurveInProgress {
    pub fn new(time_us: i64) -> CurveInProgress {
        CurveInProgress {
            curve: Arc::new(RefCell::new(Curve::new())),
            logical_start_time_us: time_us,
            wall_start_time: Instant::now(),
        }
    }

    fn elapsed_us(&self) -> i64 {
        Instant::now()
            .duration_since(self.wall_start_time)
            .as_micros()
            .try_into()
            .expect("this has been running too long!")
    }

    pub fn move_to(&mut self, p: Point) {
        self.curve.borrow_mut().move_to(p, self.elapsed_us());
    }

    pub fn line_to(&mut self, p: Point) {
        self.curve.borrow_mut().line_to(p, self.elapsed_us());
    }
}

/// This data contains the entire state of the app.
#[derive(Clone, Data, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgress>,
    pub snippets: Arc<RefCell<Snippets>>,
    pub action: CurrentAction,

    pub time_us: i64,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
   pub mouse_down: bool,
}

impl Default for ScribbleState {
    fn default() -> ScribbleState {
        ScribbleState {
            new_snippet: None,
            snippets: Arc::default(),
            action: CurrentAction::Idle,
            time_us: 0,
            mouse_down: false,
        }
    }
}

impl ScribbleState {
    pub fn curve_in_progress(&self) -> Option<std::cell::Ref<Curve>> {
        self.new_snippet.as_ref().map(|s| s.curve.borrow())
    }

    pub fn curve_in_progress_mut(&mut self) -> Option<std::cell::RefMut<Curve>> {
        self.new_snippet.as_mut().map(|s| s.curve.borrow_mut())
    }

    pub fn start_recording(&mut self) {
        assert!(self.new_snippet.is_none());
        assert_eq!(self.action, CurrentAction::Idle);
        self.new_snippet = Some(CurveInProgress::new(self.time_us));
        self.action = CurrentAction::Recording;
    }

    pub fn stop_recording(&mut self) {
        assert_eq!(self.action, CurrentAction::Recording);
        let new_snippet = self.new_snippet.take().expect("Tried to stop recording, but we hadn't started!");
        self.action = CurrentAction::Idle;
        let new_curve = new_snippet.curve.replace(Curve::new());
        if !new_curve.path.elements().is_empty() {
            self.snippets.borrow_mut().insert(new_curve);
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
    Recording,
    Playing,
    Idle,
}

impl Default for CurrentAction {
    fn default() -> CurrentAction { CurrentAction::Idle }
}

impl CurrentAction {
    pub fn rec_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            Recording => ToggledOn,
            Playing => Disabled,
            Idle => ToggledOff,
        }
    }

    pub fn play_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
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
}

