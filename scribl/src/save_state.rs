use anyhow::anyhow;
use druid::Data;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

use scribl_curves::SnippetsData;

use crate::audio::AudioSnippetsData;
use crate::EditorState;

/// This is the data that we put into the saved files.
#[derive(Clone, Data, Deserialize, Serialize)]
pub struct SaveFileData {
    /// This is the version of the save file format. Every time we change the format, this gets
    /// incremented. We retain support for reading (but not writing) old versions.
    ///
    /// The current version is 1.
    pub version: u8,

    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
}

pub mod v0 {
    #[derive(serde::Deserialize)]
    pub struct SaveFileData {
        pub version: u8,
        pub snippets: scribl_curves::save::v0::SnippetsData,
        pub audio_snippets: crate::audio::AudioSnippetsData,
    }

    impl From<SaveFileData> for super::SaveFileData {
        fn from(d: SaveFileData) -> super::SaveFileData {
            super::SaveFileData {
                version: 1,
                snippets: d.snippets.into(),
                audio_snippets: d.audio_snippets,
            }
        }
    }
}

impl SaveFileData {
    pub fn from_editor_state(data: &EditorState) -> SaveFileData {
        SaveFileData {
            version: 1,
            snippets: data.snippets.clone(),
            audio_snippets: data.audio_snippets.clone(),
        }
    }

    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<SaveFileData> {
        let file = File::open(path.as_ref())?;
        SaveFileData::load_from(file)
    }

    pub fn load_from<R: std::io::Read>(mut read: R) -> anyhow::Result<SaveFileData> {
        let mut buf = Vec::new();
        read.read_to_end(&mut buf)?;
        // The version number is at byte 9 (the first two bytes are some CBOR tags, followed by the
        // string "version", followed by the version number.
        if buf.len() < 10 {
            return Err(anyhow!("file too short!"));
        }
        let version = buf[9];
        log::info!("Found file format version {}", version);

        match version {
            0 => {
                let data: v0::SaveFileData = serde_cbor::from_slice(&buf[..])?;
                Ok(data.into())
            }
            1 => Ok(serde_cbor::from_slice(&buf[..])?),
            n => Err(anyhow!("unsupported file format version: {}", n)),
        }
    }

    pub fn save_to_path<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let path = path.as_ref();
        let tmp_file_name = format!(
            "{}.savefile",
            path.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("untitled")
        );
        let tmp_path = path.with_file_name(tmp_file_name);

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let tmp_file = File::create(&tmp_path)?;
        self.save_to(tmp_file)?;
        std::fs::rename(tmp_path, path)?;

        Ok(())
    }

    pub fn save_to<W: std::io::Write>(&self, write: W) -> anyhow::Result<()> {
        serde_cbor::to_writer(write, self)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load() {
        let data = include_bytes!("../sample/intro.scb");

        // Check that we can read our sample file.
        let save_data = SaveFileData::load_from(&data[..]).unwrap();

        let mut written = Vec::new();
        save_data.save_to(&mut written).unwrap();

        // We don't check that save -> load is the identity, because it's too
        // fragile (e.g., compression settings could change). We also don't check
        // that load -> save is the identity (for now), because implementing
        // PartialEq is a pain.
        let read_again = SaveFileData::load_from(&written[..]).unwrap();

        // We do check that if something was written using the current version
        // of scribl, then save -> load is the identity.
        let mut written_again = Vec::new();
        read_again.save_to(&mut written_again).unwrap();
        assert_eq!(written, written_again);
    }
}
