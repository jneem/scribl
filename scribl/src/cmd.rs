use druid::{FileInfo, Selector};
use std::path::PathBuf;

use scribl_curves::{DrawSnippets, Time, TimeDiff};

use crate::audio::{AudioRecordingStatus, TalkSnippet, TalkSnippets};
use crate::encode::EncodingStatus;
use crate::SaveFileData;
use crate::SnippetId;

/// Starts recording a drawing.
pub const DRAW: Selector = Selector::new("scribl.draw");

/// Starts recording audio.
pub const TALK: Selector = Selector::new("scribl.talk");

/// Starts playing.
pub const PLAY: Selector = Selector::new("scribl.play");

/// Stops recording, playing, or whatever else is going on.
pub const STOP: Selector = Selector::new("scribl.stop");

/// Selects the snippet below (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_BELOW: Selector = Selector::new("scribl.select-snippet-below");

/// Selects the snippet above (in the timeline) the currently selected snippet.
pub const SELECT_SNIPPET_ABOVE: Selector = Selector::new("scribl.select-snippet-above");

/// This command is sent by the audio thread each time it records a small chunk.
pub const RECORDING_AUDIO_STATUS: Selector<AudioRecordingStatus> =
    Selector::new("scribl.recording-audio-status");

/// Adds a new audio snippet.
pub const ADD_AUDIO_SNIPPET: Selector<TalkSnippet> = Selector::new("scribl.add-audio-snippet");

/// Truncates the currently selected snippet at the current time.
pub const TRUNCATE_SNIPPET: Selector = Selector::new("scribl.truncate-snippet");

/// Shifts the given snippet in time.
pub const SHIFT_SNIPPET: Selector<(SnippetId, TimeDiff)> = Selector::new("scribl.shift-snippet");

/// Adds a lerp to the selected snippet, lerping the current time to the marked time.
pub const LERP_SNIPPET: Selector = Selector::new("scribl.lerp-snippet");

/// Silences the selected region of the current audio snippet.
pub const SILENCE_AUDIO: Selector = Selector::new("scribl.silence-audio");

/// Deletes the selected region of the current audio snippet, "sliding" the later parts backwards.
pub const SNIP_AUDIO: Selector = Selector::new("scribl.delete-audio");

/// Changes the current animation time, assuming that the UI is in the idle state.
pub const WARP_TIME_TO: Selector<Time> = Selector::new("scribl.warp-time-to");

/// Changes the volume of the selected snippet (if it's a talk snippet).
pub const MULTIPLY_VOLUME: Selector<f64> = Selector::new("scribl.multiply-volume");

/// Exports the current animation as a video.
pub const EXPORT_CURRENT: Selector<FileInfo> = Selector::new("scribl.export-current");

/// Exports the specified animation data as a video.
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
    pub snippets: DrawSnippets,
    pub audio_snippets: TalkSnippets,
    pub filename: PathBuf,
    pub config: crate::config::Export,
}
