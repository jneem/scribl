use druid::im::HashMap;
use druid::{Data, Lens, LocalizedString, WidgetExt, WindowDesc, WindowId};

use crate::editor_state::EditorState;
use crate::menus;
use crate::widgets::Editor;

#[derive(Clone, Data, Default, Lens)]
pub struct AppState {
    next_editor_id: u32,

    // We just want a map from WindowId to the editor state, but I can't figure
    // out how to do that nicely. The issue is that in order to get the window id
    // we need to create a WindowDesc, but that requires first creating the lens
    // for the editor. So we do it in two steps.
    editors: HashMap<u32, EditorState>,
    windows: HashMap<WindowId, u32>,
}

// We can't use LensExt::Index here, because maps expect borrowed indices.
struct EditorLens(u32);

impl Lens<AppState, EditorState> for EditorLens {
    fn with<V, F: FnOnce(&EditorState) -> V>(&self, data: &AppState, f: F) -> V {
        f(&data.editors[&self.0])
    }

    fn with_mut<V, F: FnOnce(&mut EditorState) -> V>(&self, data: &mut AppState, f: F) -> V {
        f(&mut data.editors[&self.0])
    }
}

impl AppState {
    pub fn add_editor(&mut self, state: EditorState) -> WindowDesc<AppState> {
        let id = self.next_editor_id;
        self.next_editor_id += 1;

        self.editors.insert(id, state.clone());

        let ret = WindowDesc::new(move || Editor::new().lens(EditorLens(id)))
            .title(LocalizedString::new("Scribl"))
            .menu(menus::make_menu(&state))
            .window_size((800.0, 600.0));

        self.windows.insert(ret.id, id);
        ret
    }

    pub fn remove_editor(&mut self, id: WindowId) {
        if let Some(editor_id) = self.windows.get(&id) {
            self.editors.remove(&editor_id);
            log::info!("removed editor {}", editor_id);
        } else {
            log::error!("tried to remove a nonexistent editor");
        }
        self.windows.remove(&id);
    }
}
