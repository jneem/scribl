use std::collections::VecDeque;

use scribl_curves::{DrawSnippets, Time};

use crate::audio::TalkSnippets;
use crate::editor_state::{CurrentAction, SnippetId};

const MAX_UNDO_STACK: usize = 128;

/// This is the part of the editor state that gets restored when we undo/redo.
/// Note that undoing/redoing might also affect other parts of the editor state
/// (for example, if we undo while drawing, we pause the clock).
#[derive(Clone, Default)]
pub struct UndoState {
    pub snippets: DrawSnippets,
    pub audio_snippets: TalkSnippets,
    pub selected_snippet: Option<SnippetId>,
    pub mark: Option<Time>,
    pub time: Time,
    pub action: CurrentAction,
}

impl UndoState {
    pub fn with_time(mut self, time: Time) -> UndoState {
        self.time = time;
        self
    }
}

struct UndoData {
    // The state to restore when redoing this operation.
    redo_state: UndoState,
    // The state to restore when undoing this operation. This is not necessarily the same as the
    // redo state of the previous operation. For example, when undoing a draw, the time should be
    // set to that drawing snippet's start time. When redoing a draw, the time should be set to
    // whatever it was when the recording was stopped.
    undo_state: UndoState,

    // TODO: internationalization
    description: String,

    // If an undo state is transient, we delete it next time a non-transient
    // state is pushed. This is used for undoing in the middle of a snippet: we
    // store a (transient) undo state for every segment that gets drawn, and
    // then when the snippet is done being drawn we delete all the little
    // transient undo states and replace it with just one for the whole snippet.
    transient: bool,
}

#[derive(Debug)]
pub struct UndoStack {
    // Holds the stack of undo states. We push new states to the front, so `stack[0]` is the newest
    // possible state.
    stack: VecDeque<UndoData>,
    // The index of the current position in the stack. When this is zero, it means that nothing was
    // undone. When this is `stack.len()`, it means there is nothing left to undo.
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
    /// Creates a new, empty undo stack.
    pub fn new() -> UndoStack {
        UndoStack {
            stack: VecDeque::new(),
            current_state: 0,
        }
    }

    fn do_push(
        &mut self,
        undo_state: UndoState,
        redo_state: UndoState,
        description: String,
        transient: bool,
    ) {
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

        let new_state = UndoData {
            undo_state,
            redo_state,
            description,
            transient,
        };
        self.stack.push_front(new_state);
        if self.stack.len() > MAX_UNDO_STACK {
            self.stack.pop_back();
        }
        self.current_state = 0;
    }

    /// Registers a new action that can be undone. Any states that were previously undone will be
    /// forgotten.
    ///
    /// `undo_state` is the state that will be restored when the action is undone, and `redo_state`
    /// is the state that will be restored when the action is redone.
    pub fn push(&mut self, undo_state: UndoState, redo_state: UndoState, description: String) {
        self.do_push(undo_state, redo_state, description, false);
    }

    /// Registers a new transient action that can be undone. Any states that were previously undone
    /// will be forgotten.
    ///
    /// Transient actions are all forgotten whenever a non-transient action is pushed.
    ///
    /// `undo_state` is the state that will be restored when the action is undone, and `redo_state`
    /// is the state that will be restored when the action is redone.
    pub fn push_transient(
        &mut self,
        undo_state: UndoState,
        redo_state: UndoState,
        description: String,
    ) {
        self.do_push(undo_state, redo_state, description, true);
    }

    /// If there is a most recent action to undo, rewinds the undo stack to that action and returns
    /// the state that should be restored.
    pub fn undo(&mut self) -> Option<UndoState> {
        if self.current_state < self.stack.len() {
            let state = self.stack[self.current_state].undo_state.clone();
            self.current_state += 1;
            Some(state)
        } else {
            None
        }
    }

    /// Returns a description of the action that can be undone.
    pub fn undo_description(&self) -> Option<&str> {
        self.stack
            .get(self.current_state)
            .map(|undo| undo.description.as_str())
    }

    /// If there is an undone action that can be redone, advances the undo stack to that action and
    /// returns the state that should be restored.
    pub fn redo(&mut self) -> Option<UndoState> {
        if self.current_state > 0 {
            self.current_state -= 1;
            Some(self.stack[self.current_state].redo_state.clone())
        } else {
            None
        }
    }

    /// Returns a description of the action that can be redone.
    pub fn redo_description(&self) -> Option<&str> {
        if self.current_state > 0 {
            Some(&self.stack[self.current_state - 1].description)
        } else {
            None
        }
    }

    /// Returns `true` if there is an action to undo.
    pub fn can_undo(&self) -> bool {
        self.current_state < self.stack.len()
    }

    /// Returns `true` if there is an action to redo.
    pub fn can_redo(&self) -> bool {
        self.current_state > 0
    }
}
