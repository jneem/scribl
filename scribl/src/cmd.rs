use druid::{FileInfo, Selector};
use std::path::PathBuf;

use scribl_curves::Time;

use crate::audio::{AudioRecordingStatus, TalkSnippet};
use crate::encode::EncodingStatus;
use crate::{SaveFileData, ScriblState};

/// Selects the snippet below (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_BELOW: Selector = Selector::new("scribl.select-snippet-below");

/// Selects the snippet above (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_ABOVE: Selector = Selector::new("scribl.select-snippet-above");

/// This command is sent by the audio thread each time it records a small chunk.
pub const RECORDING_AUDIO_STATUS: Selector<AudioRecordingStatus> =
    Selector::new("scribl.recording-audio-status");

/// Adds a new audio snippet.
pub const ADD_TALK_SNIPPET: Selector<TalkSnippetCmd> = Selector::new("scribl.add-talk-snippet");

/// Changes the current animation time, assuming that the UI is in the idle state.
pub const WARP_TIME_TO: Selector<Time> = Selector::new("scribl.warp-time-to");

/// Exports the current animation as a video.
pub const EXPORT: Selector<FileInfo> = Selector::new("scribl.export");

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
    pub scribl: ScriblState,
    pub filename: PathBuf,
    pub config: crate::config::Export,
}

pub struct TalkSnippetCmd {
    pub snip: TalkSnippet,
    /// The start time of the talk snippet *before* it got trimmed.
    pub orig_start: Time,
}
