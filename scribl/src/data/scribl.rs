use druid::{Data, Lens};
use scribl_curves::{DrawSnippet, DrawSnippetId, DrawSnippets};

use crate::audio::{TalkSnippet, TalkSnippetId, TalkSnippets};
use crate::undo::UndoState;
use crate::SaveFileData;

/// This data contains the state of the current scribl. That means, just the parts that get saved
/// if we save the file.
#[derive(Clone, Data, Default, Lens)]
pub struct ScriblState {
    pub draw: DrawSnippets,
    pub talk: TalkSnippets,
}

impl ScriblState {
    pub fn new(draw: DrawSnippets, talk: TalkSnippets) -> ScriblState {
        ScriblState { draw, talk }
    }

    pub fn from_save_file(data: &SaveFileData) -> ScriblState {
        ScriblState {
            draw: data.snippets.clone(),
            talk: data.audio_snippets.clone(),
        }
    }

    pub fn add_draw_snippet(&mut self, snip: DrawSnippet) -> DrawSnippetId {
        let (new_snippets, new_id) = self.draw.with_new_snippet(snip);
        self.draw = new_snippets;
        new_id
    }

    pub fn add_talk_snippet(&mut self, snip: TalkSnippet) -> TalkSnippetId {
        let (new_snippets, new_id) = self.talk.with_new_snippet(snip);
        self.talk = new_snippets;
        new_id
    }

    pub fn delete_draw_snippet(&mut self, id: DrawSnippetId) {
        self.draw = self.draw.without_snippet(id);
    }

    pub fn delete_talk_snippet(&mut self, id: TalkSnippetId) {
        self.talk = self.talk.without_snippet(id);
    }

    pub fn restore_undo_state(&mut self, undo: &UndoState) {
        self.draw = undo.snippets.clone();
        self.talk = undo.audio_snippets.clone();
    }
}
