use druid::{AppDelegate, Command, DelegateCtx, Env, FileInfo, Target, WindowId};
use std::fs;
use std::fs::File;
use std::path::Path;

use crate::data::{AppState, SaveFileData};

#[derive(Debug, Default)]
pub struct Delegate;

fn save_file(path: &Path, data: &AppState) -> std::io::Result<()> {
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
    serde_json::to_writer(tmp_file, &data.scribble.to_save_file()).unwrap(); // FIXME: unwrap
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
                dbg!(target);
                if let Ok(info) = cmd.get_object::<FileInfo>() {
                    data.save_path = Some(info.path().to_owned());
                }
                let path = data.save_path.as_ref().expect("no save path");
                if let Err(e) = save_file(path, data) {
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
