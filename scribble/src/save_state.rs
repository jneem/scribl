use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::Path;

use scribble_curves::SnippetsData;

use crate::audio::AudioSnippetsData;

/// Our save file format is simply to serialize this struct as CBOR.
///
/// In particular, it's very important that the serializion format of this struct
/// doesn't change unexpectedly.
#[derive(Deserialize, Serialize)]
pub struct SaveFileData {
    /// This is currently always set to zero, but it's here in case we need to make
    /// changes.
    pub version: u64,

    pub snippets: SnippetsData,
    pub audio_snippets: AudioSnippetsData,
}

impl SaveFileData {
    pub fn load_from_path<P: AsRef<Path>>(path: P) -> anyhow::Result<SaveFileData> {
        let file = File::open(path.as_ref())?;
        SaveFileData::load_from(file)
    }

    pub fn load_from<R: std::io::Read>(read: R) -> anyhow::Result<SaveFileData> {
        Ok(serde_cbor::from_reader(read)?)
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
        // of scribble, then save -> load is the identity.
        let mut written_again = Vec::new();
        read_again.save_to(&mut written_again).unwrap();
        assert_eq!(written, written_again);
    }
}
