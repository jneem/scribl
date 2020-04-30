// Our general undo philosophy follows the data split between `AppState` and
// `ScribbleState`: the latter contains the state of the actual animation being
// created, and the changes to that are the ones that we want to support
// undoing. Therefore, our undo stack is essentially just a stack of
// `ScribbleState`s, and we execute undoing and redoing by pushing and popping
// these states. (This is less wasteful than it seems at first glance, because
// most of the actual data in `ScribbleState` is behind shared pointers.)
//
// In this module, we don't see the application state at all, but it is still
// relevant to the bigger undo picture, because an undo/redo command might want
// to change the `AppState` in addition to restoring its `ScribbleState`. For
// example, it might want to stop playback or pause recording.

use std::collections::VecDeque;

use crate::data::ScribbleState;

const MAX_UNDO_STACK: usize = 128;

struct UndoData {
    scribble: ScribbleState,

    // If an undo state is transient, we delete it next time a non-transient
    // state is pushed. This is used for undoing in the middle of a snippet: we
    // store a (transient) undo state for every segment that gets drawn, and
    // then when the snippet is done being drawn we delete all the little
    // transient undo states and replace it with just one for the whole snippet.
    transient: bool,
}

#[derive(Debug)]
pub struct UndoStack {
    stack: VecDeque<UndoData>,
    current_state: usize,
}

impl std::fmt::Debug for UndoData {
    fn fmt(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.debug_struct("UndoData")
            .field("transient", &self.transient)
            .finish()
    }
}

impl UndoStack {
    pub fn new(initial_state: ScribbleState) -> UndoStack {
        let mut stack = VecDeque::new();
        stack.push_front(UndoData {
            scribble: initial_state,
            transient: false,
        });
        UndoStack {
            stack,
            current_state: 0,
        }
    }

    fn do_push(&mut self, state: &ScribbleState, transient: bool) {
        // In case the current state is not the newest one, remove all the newer ones from the
        // stack.
        self.stack.drain(0..self.current_state);

        // In case the top of the stack is transient and this one isn't, remove all the transient ones.
        if !transient {
            dbg!(&self);
            let last_permanent = self
                .stack
                .iter()
                .position(|s| !s.transient)
                .unwrap_or(self.stack.len());
            dbg!(last_permanent);
            self.stack.drain(..last_permanent);
        }

        let new_state = UndoData {
            scribble: state.clone(),
            transient,
        };
        self.stack.push_front(new_state);
        if self.stack.len() > MAX_UNDO_STACK {
            self.stack.pop_back();
        }
        self.current_state = 0;
    }
    pub fn push(&mut self, state: &ScribbleState) {
        self.do_push(state, false);
    }

    pub fn push_transient(&mut self, state: &ScribbleState) {
        self.do_push(state, true);
    }

    pub fn undo(&mut self) -> Option<ScribbleState> {
        if self.current_state + 1 < self.stack.len() {
            self.current_state += 1;
            Some(self.stack[self.current_state].scribble.clone())
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<ScribbleState> {
        if self.current_state > 0 {
            self.current_state -= 1;
            Some(self.stack[self.current_state].scribble.clone())
        } else {
            None
        }
    }
}
