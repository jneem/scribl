use druid::{Data, Lens};
use scribl_curves::DrawSnippets;

use crate::audio::TalkSnippets;

/// This data contains the state of the current scribl. That means, just the parts that get saved
/// if we save the file.
#[derive(Clone, Data, Lens)]
pub struct ScriblState {
    pub draw: DrawSnippets,
    pub talk: TalkSnippets,
}

impl ScriblState {
    pub fn new(draw: DrawSnippets, talk: TalkSnippets) -> ScriblState {
        ScriblState { draw, talk }
    }
}
