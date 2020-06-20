use druid::{Color, ExtEventSink, Selector};
use std::path::PathBuf;

use scribl_curves::{SnippetData, SnippetsData, Time};

use crate::audio::{AudioSnippetData, AudioSnippetsData};
use crate::editor_state::{MaybeSnippetId, StrokeInProgress};
use crate::encode::EncodingStatus;
use crate::save_state::SaveFileData;

/// Starts recording a drawing.
pub const DRAW: Selector = Selector::new("scribl.draw");

/// Starts recording audio.
pub const TALK: Selector = Selector::new("scribl.talk");

/// Starts playing.
pub const PLAY: Selector = Selector::new("scribl.play");

/// Stops recording, playing, or whatever else is going on.
pub const STOP: Selector = Selector::new("scribl.stop");

/// Adds a new snippet.
pub const ADD_SNIPPET: Selector<SnippetData> = Selector::new("scribl.add-snippet");

/// Deletes a snipppet. If the argument is `None`, the currently selected snippet is deleted.
pub const DELETE_SNIPPET: Selector<MaybeSnippetId> = Selector::new("scribl.delete-snippet");

/// Adds a new audio snippet.
pub const ADD_AUDIO_SNIPPET: Selector<AudioSnippetData> = Selector::new("scribl.add-audio-snippet");

/// Truncates the currently selected snippet at the current time.
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribl.truncate-snippet");

/// Adds a lerp to the selected snippet, lerping the current time to the marked time.
pub const LERP_SNIPPET: Selector = Selector::new("scribl.lerp-snippet");

/// Changes the current mark time. If the argument is `None`, the current time will be used
/// instead.
pub const SET_MARK: Selector<Option<Time>> = Selector::new("scribl.set-mark");

/// Changes the current animation time.
pub const WARP_TIME_TO: Selector<Time> = Selector::new("scribl.warp-time-to");

/// Changes the pen color.
pub const CHOOSE_COLOR: Selector<Color> = Selector::new("scribl.choose-color");

/// Exports the current animation as a video.
pub const EXPORT: Selector<ExportCmd> = Selector::new("scribl.export");

/// Appends a new segment to the currently-drawing snippet.
pub const APPEND_NEW_SEGMENT: Selector<StrokeInProgress> =
    Selector::new("scribl.append-new-segment");

/// While the video is encoding asynchronously, it periodically sends these commands.
pub const ENCODING_STATUS: Selector<EncodingStatus> = Selector::new("scribl.encoding-status");

/// Reading and parsing of save-files is done asynchronously. When a file is done being read and
/// parsed, one of these commands gets sent.
pub const FINISHED_ASYNC_LOAD: Selector<Result<SaveFileData, anyhow::Error>> =
    Selector::new("scribl.finished-async-load");

/// Writing save-files is done asynchronously. When a file is done being written one of these
/// commands gets sent.
pub const FINISHED_ASYNC_SAVE: Selector<AsyncSaveResult> =
    Selector::new("scribl.finished-async-save");

/// This command provides an `ExtEventSink` to widgets that want one.
pub const INITIALIZE_EVENT_SINK: Selector<ExtEventSink> =
    Selector::new("scribl.initialize-event-sink");

#[derive(Clone)]
pub struct AsyncSaveResult {
    path: PathBuf,
    error: Option<String>,
    autosave: bool,
}

#[derive(Clone)]
pub struct ExportCmd {
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub filename: PathBuf,
}
