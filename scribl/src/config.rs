use anyhow::{anyhow, Context, Result};
use directories_next::ProjectDirs;
use serde::Deserialize;

fn default_video_height() -> u32 {
    1080
}

fn default_video_fps() -> f64 {
    30.0
}

#[derive(Clone, Debug, Deserialize)]
pub struct Config {
    pub export: Export,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Export {
    /// Height of the exported video, in pixels.
    #[serde(default = "default_video_height")]
    pub height: u32,

    /// Frames per second in the exported video.
    #[serde(default = "default_video_fps")]
    pub fps: f64,
}

impl Default for Config {
    fn default() -> Config {
        Config {
            export: Export {
                height: default_video_height(),
                fps: default_video_fps(),
            },
        }
    }
}

fn do_load_config() -> Result<Config> {
    if let Some(proj_dirs) = ProjectDirs::from("ink", "scribl", "scribl") {
        let mut path = proj_dirs.config_dir().to_owned();
        path.push("config.toml");
        let data = std::fs::read_to_string(&path).context(format!("config path {:?}", path))?;
        let conf = toml::from_str(&data)?;
        Ok(conf)
    } else {
        Err(anyhow!("couldn't determine config directory"))
    }
}

pub fn load_config() -> Config {
    match do_load_config() {
        Err(e) => {
            log::info!("Failed to load config: {}", e);
            Config::default()
        }
        Ok(c) => {
            log::info!("Loaded configuration: {:?}", c);
            c
        }
    }
}
