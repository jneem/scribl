use druid::Selector;
use std::path::PathBuf;

use crate::audio::AudioSnippetsData;
use crate::data::SnippetsData;
use crate::snippet::SnippetId;

/// Adds a new snippet. The argument is a [`SnippetData`].
pub const ADD_SNIPPET: Selector = Selector::new("scribble.add-snippet");

/// Deletes a snipppet. The argument is a [`SnippetId`].
pub const DELETE_SNIPPET: Selector = Selector::new("scribble.delete-snippet");

/// Adds a new audio snippet. The argument is an [`AudioSnippetData`].
pub const ADD_AUDIO_SNIPPET: Selector = Selector::new("scribble.add-audio-snippet");

/// Deletes an audio snipppet. The argument is an [`AudioSnippetId`].
pub const DELETE_AUDIO_SNIPPET: Selector = Selector::new("scribble.delete-audio-snippet");

/// Truncates a snippet. The argument is a [`TruncateSnippetCmd`].
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribble.truncate-snippet");

/// Adds a lerp to a snippet. The argument is a [`LerpSnippetCmd`].
pub const LERP_SNIPPET: Selector = Selector::new("scribble.lerp-snippet");

/// Changes the current mark time. The argument is an i64.
pub const SET_MARK: Selector = Selector::new("scribble.set-mark");

/// Changes the pen color. The argument is a [`Color`].
pub const CHOOSE_COLOR: Selector = Selector::new("scribble.choose-color");

/// Exports the current animation as a video. The argument is an [`ExportCmd`].
pub const EXPORT: Selector = Selector::new("scribble.export");

pub struct TruncateSnippetCmd {
    pub id: SnippetId,
    pub time_us: i64,
}

pub struct LerpSnippetCmd {
    pub id: SnippetId,
    pub from_time: i64,
    pub to_time: i64,
}

pub struct ExportCmd {
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub filename: PathBuf,
}
