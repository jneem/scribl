use druid::{AppDelegate, Command, DelegateCtx, Env, FileInfo, Target, WindowId};

use crate::cmd;
use crate::data::{AppState, SaveFileData};

#[derive(Debug, Default)]
pub struct Delegate;

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> bool {
        match cmd.selector {
            druid::commands::SAVE_FILE => {
                let path = if let Ok(info) = cmd.get_object::<FileInfo>() {
                    info.path().to_owned()
                } else if let Some(path) = data.save_path.as_ref() {
                    path.to_owned()
                } else {
                    log::error!("no save path, not saving");
                    return false;
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
                        if let Err(e) = data.scribble.to_save_file().save_to(&path) {
                            log::error!("error saving: '{}'", e);
                        }
                    }
                    _ => {
                        log::error!("unknown extension! Trying to save anyway");
                        data.save_path = Some(path.clone());
                        if let Err(e) = data.scribble.to_save_file().save_to(&path) {
                            log::error!("error saving: '{}'", e);
                        }
                    }
                }
                ctx.submit_command(cmd::REBUILD_MENUS, target);
                false
            }
            druid::commands::OPEN_FILE => {
                let info = if let Ok(info) = cmd.get_object::<FileInfo>() {
                    info
                } else {
                    log::error!("no open file info, not opening");
                    return false;
                };
                match SaveFileData::load_from(info.path()) {
                    Ok(save_data) => {
                        *data = AppState::from_save_file(save_data);
                    }
                    Err(e) => {
                        log::error!("error loading: '{}'", e);
                    }
                }
                ctx.submit_command(cmd::REBUILD_MENUS, target);
                false
            }
            cmd::REBUILD_MENUS => {
                ctx.submit_command(
                    Command::new(druid::commands::SET_MENU, crate::menus::make_menu(data)),
                    target,
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
