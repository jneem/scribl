use druid::{AppDelegate, Command, DelegateCtx, Env, Target, WindowId};

use crate::app_state::AppState;
use crate::editor_state::EditorState;

#[derive(Debug, Default)]
pub struct Delegate;

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> bool {
        log::info!("command {:?}", cmd);
        if cmd.is(druid::commands::NEW_FILE) {
            let window_desc = data.add_editor(EditorState::default());
            ctx.new_window(window_desc);
            false
        } else {
            true
        }
    }

    fn window_removed(
        &mut self,
        id: WindowId,
        data: &mut AppState,
        _env: &Env,
        _ctx: &mut DelegateCtx,
    ) {
        data.remove_editor(id);
    }
}
