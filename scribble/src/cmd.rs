use druid::{Color, Selector};
use std::path::PathBuf;

use scribble_curves::{SnippetData, SnippetsData, Time};

use crate::audio::{AudioSnippetData, AudioSnippetsData};
use crate::editor_state::{MaybeSnippetId, SegmentInProgress};

/// Starts recording a drawing.
pub const DRAW: Selector = Selector::new("scribble.draw");

/// Starts recording audio.
pub const TALK: Selector = Selector::new("scribble.talk");

/// Starts playing.
pub const PLAY: Selector = Selector::new("scribble.play");

/// Stops recording, playing, or whatever else is going on.
pub const STOP: Selector = Selector::new("scribble.stop");

/// Adds a new snippet.
pub const ADD_SNIPPET: Selector<SnippetData> = Selector::new("scribble.add-snippet");

/// Deletes a snipppet. If the argument is `None`, the currently selected snippet is deleted.
pub const DELETE_SNIPPET: Selector<MaybeSnippetId> = Selector::new("scribble.delete-snippet");

/// Adds a new audio snippet.
pub const ADD_AUDIO_SNIPPET: Selector<AudioSnippetData> =
    Selector::new("scribble.add-audio-snippet");

/// Truncates the currently selected snippet at the current time.
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribble.truncate-snippet");

/// Adds a lerp to the selected snippet, lerping the current time to the marked time.
pub const LERP_SNIPPET: Selector = Selector::new("scribble.lerp-snippet");

/// Changes the current mark time. If the argument is `None`, the current time will be used
/// instead.
pub const SET_MARK: Selector<Option<Time>> = Selector::new("scribble.set-mark");

/// Changes the current animation time.
pub const WARP_TIME_TO: Selector<Time> = Selector::new("scribble.warp-time-to");

/// Changes the pen color.
pub const CHOOSE_COLOR: Selector<Color> = Selector::new("scribble.choose-color");

/// Exports the current animation as a video.
pub const EXPORT: Selector<ExportCmd> = Selector::new("scribble.export");

/// Appends a new segment to the currently-drawing snippet.
pub const APPEND_NEW_SEGMENT: Selector<SegmentInProgress> =
    Selector::new("scribble.append-new-segment");

#[derive(Clone)]
pub struct ExportCmd {
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub filename: PathBuf,
}
