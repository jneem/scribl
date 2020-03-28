use std::collections::VecDeque;

use crate::data::ScribbleState;

const MAX_UNDO_STACK: usize = 128;

pub struct UndoStack {
    stack: VecDeque<ScribbleState>,
    current_state: usize,
}

impl UndoStack {
    pub fn new(initial_state: ScribbleState) -> UndoStack {
        let mut stack = VecDeque::new();
        stack.push_front(initial_state);
        UndoStack {
            stack,
            current_state: 0,
        }
    }

    pub fn push(&mut self, state: &ScribbleState) {
        // In case the current state is not the newest one, remove all the newer ones from the
        // stack.
        self.stack.drain(0..self.current_state);
        self.stack.push_front(state.clone());
        if self.stack.len() > MAX_UNDO_STACK {
            self.stack.pop_back();
        }
        self.current_state = 0;
    }

    pub fn undo(&mut self) -> Option<ScribbleState> {
        if self.current_state + 1 < self.stack.len() {
            self.current_state += 1;
            Some(self.stack[self.current_state].clone())
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<ScribbleState> {
        if self.current_state > 0 {
            self.current_state -= 1;
            Some(self.stack[self.current_state].clone())
        } else {
            None
        }
    }
}
