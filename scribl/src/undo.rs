use std::collections::VecDeque;
use std::sync::Arc;

use scribl_curves::{SnippetsData, StrokeSeq, Time};

use crate::audio::AudioSnippetsData;
use crate::editor_state::MaybeSnippetId;

const MAX_UNDO_STACK: usize = 128;

/// This is the part of the editor state that gets restored when we undo/redo.
/// Note that undoing/redoing might also affect other parts of the editor state
/// (for example, if we undo while drawing, we pause the clock).
#[derive(Clone, Default)]
pub struct UndoState {
    pub new_curve: Option<Arc<StrokeSeq>>,
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub selected_snippet: MaybeSnippetId,
    pub mark: Option<Time>,
    pub time: Time,
}

struct UndoData {
    state: UndoState,

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
    pub fn new(initial_state: UndoState) -> UndoStack {
        let mut stack = VecDeque::new();
        stack.push_front(UndoData {
            state: initial_state,
            transient: false,
        });
        UndoStack {
            stack,
            current_state: 0,
        }
    }

    fn do_push(&mut self, state: UndoState, transient: bool) {
        // In case the current state is not the newest one, remove all the newer ones from the
        // stack.
        self.stack.drain(0..self.current_state);

        // In case the top of the stack is transient and this one isn't, remove all the transient ones.
        if !transient {
            let last_permanent = self
                .stack
                .iter()
                .position(|s| !s.transient)
                .unwrap_or(self.stack.len());
            self.stack.drain(..last_permanent);
        }

        let new_state = UndoData { state, transient };
        self.stack.push_front(new_state);
        if self.stack.len() > MAX_UNDO_STACK {
            self.stack.pop_back();
        }
        self.current_state = 0;
    }
    pub fn push(&mut self, state: UndoState) {
        self.do_push(state, false);
    }

    pub fn push_transient(&mut self, state: UndoState) {
        self.do_push(state, true);
    }

    pub fn undo(&mut self) -> Option<UndoState> {
        if self.current_state + 1 < self.stack.len() {
            self.current_state += 1;
            Some(self.stack[self.current_state].state.clone())
        } else {
            None
        }
    }

    pub fn redo(&mut self) -> Option<UndoState> {
        if self.current_state > 0 {
            self.current_state -= 1;
            Some(self.stack[self.current_state].state.clone())
        } else {
            None
        }
    }

    pub fn can_undo(&self) -> bool {
        self.current_state + 1 < self.stack.len()
    }

    pub fn can_redo(&self) -> bool {
        self.current_state > 0
    }
}
