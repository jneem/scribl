use druid::Selector;
use std::path::PathBuf;

use scribble_curves::SnippetsData;

use crate::audio::AudioSnippetsData;

/// Starts recording a drawing. There is no argument.
pub const DRAW: Selector = Selector::new("scribble.draw");

/// Starts recording audio. There is no argument.
pub const TALK: Selector = Selector::new("scribble.talk");

/// Starts playing. There is no argument.
pub const PLAY: Selector = Selector::new("scribble.play");

/*
/// Pauses an animation. There is no argument.
pub const PAUSE: Selector = Selector::new("scribble.pause");
*/

/// Stops recording, playing, or whatever else is going on.
pub const STOP: Selector = Selector::new("scribble.stop");

/// Adds a new snippet. The argument is a [`SnippetData`].
pub const ADD_SNIPPET: Selector = Selector::new("scribble.add-snippet");

/// Deletes a snipppet. The argument is an optional [`SnippetId`] or
/// [`AudioSnippetId`]. If there is no argument, the currently selected snippet
/// is deleted.
pub const DELETE_SNIPPET: Selector = Selector::new("scribble.delete-snippet");

/// Adds a new audio snippet. The argument is an [`AudioSnippetData`].
pub const ADD_AUDIO_SNIPPET: Selector = Selector::new("scribble.add-audio-snippet");

/// Truncates the currently selected snippet at the current time. There is no
/// argument.
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribble.truncate-snippet");

/// Adds a lerp to the selected snippet, lerping the current time to the marked time.
pub const LERP_SNIPPET: Selector = Selector::new("scribble.lerp-snippet");

/// Changes the current mark time. The argument is an optional [`Time`]. If it is
/// not present, the current time will be used instead.
pub const SET_MARK: Selector = Selector::new("scribble.set-mark");

/// Changes the current animation time. The argument is a [`Time`].
pub const WARP_TIME_TO: Selector = Selector::new("scribble.warp-time-to");

/// Changes the pen color. The argument is a [`Color`].
pub const CHOOSE_COLOR: Selector = Selector::new("scribble.choose-color");

/// Exports the current animation as a video. The argument is an [`ExportCmd`].
pub const EXPORT: Selector = Selector::new("scribble.export");

/// Appends a new segment to the currently-drawing snippet. The argument is a [`SegmentInProgress`].
pub const APPEND_NEW_SEGMENT: Selector = Selector::new("scribble.append-new-segment");

/// Recreate the menus. There is no argument.
pub const REBUILD_MENUS: Selector = Selector::new("scribble.rebuild-menus");

#[derive(Clone)]
pub struct ExportCmd {
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub filename: PathBuf,
}
