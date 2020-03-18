use druid::{Data, Lens, Point};
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
}

/// This data contains the entire state of the app.
#[derive(Clone, Data, Default, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgress>,
    pub snippets: Arc<Snippets>,
    pub action: CurrentAction,

    pub time_us: i64,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
   pub mouse_down: bool,
}

impl ScribbleState {
    pub fn curve_in_progress(&self) -> Option<std::cell::Ref<Curve>> {
        self.new_snippet.as_ref().map(|s| s.curve.borrow())
    }

    pub fn curve_in_progress_mut(&mut self) -> Option<std::cell::RefMut<Curve>> {
        self.new_snippet.as_mut().map(|s| s.curve.borrow_mut())
    }
}

impl CurveInProgress {
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

    pub fn initialize_if_necessary(&mut self, time_us: i64) {
        if self.curve.borrow().path.elements().is_empty() {
            dbg!("Starting recording at time {}", time_us);
            self.logical_start_time_us = time_us;
            self.wall_start_time = std::time::Instant::now();
        }
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
    pub fn get_rec_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            Recording => ToggledOn,
            Playing => Disabled,
            Idle => ToggledOff,
        }
    }

    pub fn put_rec_toggle(&mut self, t: ToggleButtonState) {
        use CurrentAction::*;
        use ToggleButtonState::*;
        *self = match t {
            ToggledOn => Recording,
            ToggledOff => Idle,
            Disabled => Playing,
        };
    }

    pub fn is_recording(&self) -> bool {
        *self == CurrentAction::Recording
    }
}

