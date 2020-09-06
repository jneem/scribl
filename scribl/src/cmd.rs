use druid::{Color, ExtEventSink, Selector};
use std::path::PathBuf;

use scribl_curves::{SnippetData, SnippetsData, Time};

use crate::audio::{AudioRecordingStatus, AudioSnippetData, AudioSnippetsData};
use crate::editor_state::MaybeSnippetId;
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

/// Selects the snippet below (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_BELOW: Selector = Selector::new("scribl.select-snippet-below");

/// Selects the snippet above (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_ABOVE: Selector = Selector::new("scribl.select-snippet-above");

/// This command is sent by the audio thread each time it records a small chunk.
pub const RECORDING_AUDIO_STATUS: Selector<AudioRecordingStatus> =
    Selector::new("scribl.recording-audio-status");

/// Adds a new audio snippet.
pub const ADD_AUDIO_SNIPPET: Selector<AudioSnippetData> = Selector::new("scribl.add-audio-snippet");

/// Truncates the currently selected snippet at the current time.
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribl.truncate-snippet");

/// Adds a lerp to the selected snippet, lerping the current time to the marked time.
pub const LERP_SNIPPET: Selector = Selector::new("scribl.lerp-snippet");

/// Changes the current mark time. If the argument is `None`, the current time will be used
/// instead.
pub const SET_MARK: Selector<Option<Time>> = Selector::new("scribl.set-mark");

/// Changes the current animation time, assuming that the UI is in the idle state.
pub const WARP_TIME_TO: Selector<Time> = Selector::new("scribl.warp-time-to");

/// Updates the current animation time based on the system clock. This gets sent repeatedly while
/// animating.
pub const UPDATE_TIME: Selector<()> = Selector::new("scribl.update-time");

/// Changes the pen color.
pub const CHOOSE_COLOR: Selector<Color> = Selector::new("scribl.choose-color");

/// Exports the current animation as a video.
pub const EXPORT: Selector<ExportCmd> = Selector::new("scribl.export");

/// While the video is encoding asynchronously, it periodically sends these commands.
pub const ENCODING_STATUS: Selector<EncodingStatus> = Selector::new("scribl.encoding-status");

/// Reading and parsing of save-files is done asynchronously. When a file is done being read and
/// parsed, one of these commands gets sent.
pub const FINISHED_ASYNC_LOAD: Selector<AsyncLoadResult> =
    Selector::new("scribl.finished-async-load");

/// Writing save-files is done asynchronously. When a file is done being written one of these
/// commands gets sent.
pub const FINISHED_ASYNC_SAVE: Selector<AsyncSaveResult> =
    Selector::new("scribl.finished-async-save");

/// This command provides an `ExtEventSink` to widgets that want one.
pub const INITIALIZE_EVENT_SINK: Selector<ExtEventSink> =
    Selector::new("scribl.initialize-event-sink");

pub const ZOOM_IN: Selector<()> = Selector::new("scribl.zoom-in");
pub const ZOOM_OUT: Selector<()> = Selector::new("scribl.zoom-out");
pub const ZOOM_RESET: Selector<()> = Selector::new("scribl.zoom-reset");

#[derive(Clone)]
pub struct AsyncLoadResult {
    pub path: PathBuf,
    pub save_data: Result<SaveFileData, String>,
}

#[derive(Clone)]
pub struct AsyncSaveResult {
    pub path: PathBuf,
    pub data: SaveFileData,
    pub error: Option<String>,
    pub autosave: bool,
}

#[derive(Clone)]
pub struct ExportCmd {
    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
    pub filename: PathBuf,
    pub config: crate::config::Export,
}
