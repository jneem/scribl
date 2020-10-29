use druid::{AppDelegate, Command, DelegateCtx, Env, Handled, Target, WindowId};

use crate::app_state::AppState;
use crate::editor_state::EditorState;

#[derive(Default)]
pub struct Delegate {}

impl AppDelegate<AppState> for Delegate {
    fn command(
        &mut self,
        ctx: &mut DelegateCtx,
        _target: Target,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> Handled {
        log::info!("command {:?}", cmd);
        if cmd.is(druid::commands::NEW_FILE) {
            let window_desc = data.add_editor(EditorState::default());
            ctx.new_window(window_desc);
            Handled::Yes
        } else {
            Handled::No
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
