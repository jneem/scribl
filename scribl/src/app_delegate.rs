use druid::{AppDelegate, Command, DelegateCtx, Env, ExtEventSink, Target, WindowId};

use crate::app_state::AppState;
use crate::editor_state::EditorState;

pub struct Delegate {
    ext_handle: ExtEventSink,
}

impl Delegate {
    pub fn new(ext_handle: ExtEventSink) -> Delegate {
        Delegate { ext_handle }
    }
}

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
            let id = window_desc.id;
            ctx.new_window(window_desc);

            // Plumb the ExtEventSink into the new window.
            if let Err(e) = self.ext_handle.submit_command(
                crate::cmd::INITIALIZE_EVENT_SINK,
                self.ext_handle.clone(),
                id,
            ) {
                log::error!(
                    "failed to initialize event sink, loading files won't work: {}",
                    e
                );
            }

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
