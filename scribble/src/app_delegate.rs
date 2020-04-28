use druid::{AppDelegate, Command, DelegateCtx, Env, FileInfo, Target, WindowId};
use std::fs;
use std::fs::File;
use std::path::{Path, PathBuf};

use crate::cmd;
use crate::data::{AppState, SaveFileData};

#[derive(Debug, Default)]
pub struct Delegate;

fn save_file<S: serde::Serialize>(path: &Path, data: &S) -> std::io::Result<()> {
    let tmp_file_name = format!(
        "{}.savefile",
        path.file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("untitled")
    );
    let tmp_path = path.with_file_name(tmp_file_name);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let tmp_file = File::create(&tmp_path)?;
    serde_json::to_writer(tmp_file, data).unwrap(); // FIXME: unwrap
    fs::rename(tmp_path, path)?;

    Ok(())
}

fn load_file(path: &Path) -> std::io::Result<SaveFileData> {
    let file = File::open(path)?;
    let ret = serde_json::from_reader(file).unwrap(); // FIXME: unwrap
    Ok(ret)
}

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        target: &Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> bool {
        match cmd.selector {
            druid::commands::SAVE_FILE => {
                let path = if let Ok(info) = cmd.get_object::<FileInfo>() {
                    info.path().to_owned()
                } else {
                    data.save_path.as_ref().expect("no save path").to_owned()
                };

                // Note that we use the SAVE_FILE command for both saving and
                // exporting, and we decide which to do based on the file
                // extension.
                match path.extension().and_then(|e| e.to_str()) {
                    Some("mp4") => {
                        let export = cmd::ExportCmd {
                            snippets: data.scribble.snippets.clone(),
                            audio_snippets: data.scribble.audio_snippets.clone(),
                            filename: path.to_owned(),
                        };
                        ctx.submit_command(Command::new(cmd::EXPORT, export), None);
                    }
                    Some("scb") => {
                        data.save_path = Some(path.clone());
                        if let Err(e) = save_file(&path, &data.scribble.to_save_file()) {
                            log::error!("error saving: '{}'", e);
                        }
                    }
                    _ => {
                        log::error!("unknown extension! Trying to save anyway");
                        data.save_path = Some(path.clone());
                        if let Err(e) = save_file(&path, &data.scribble.to_save_file()) {
                            log::error!("error saving: '{}'", e);
                        }
                    }
                }
                ctx.submit_command(
                    Command::new(druid::commands::SET_MENU, crate::menus::make_menu(data)),
                    *target,
                );
                false
            }
            cmd::SAVE_ANIM_ONLY => {
                let path = cmd.get_object::<PathBuf>().expect("API violation");
                if let Err(e) = save_file(&path, &data.scribble.snippets) {
                    log::error!("error saving: '{}'", e);
                }
                ctx.submit_command(
                    Command::new(druid::commands::SET_MENU, crate::menus::make_menu(data)),
                    *target,
                );
                false
            }
            druid::commands::OPEN_FILE => {
                let info = cmd.get_object::<FileInfo>().expect("no file info");
                match load_file(info.path()) {
                    Ok(save_data) => {
                        *data = AppState::from_save_file(save_data);
                    }
                    Err(e) => {
                        log::error!("error loading: '{}'", e);
                    }
                }
                ctx.submit_command(
                    Command::new(druid::commands::SET_MENU, crate::menus::make_menu(data)),
                    *target,
                );
                false
            }
            _ => true,
        }
    }

    fn window_removed(
        &mut self,
        _id: WindowId,
        _data: &mut AppState,
        _env: &Env,
        _ctx: &mut DelegateCtx,
    ) {
        log::info!("window removed");
    }
}
