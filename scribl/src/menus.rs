use druid::commands;
use druid::menu::MenuEventCtx;
use druid::platform_menus;
use druid::{
    Env, FileDialogOptions, FileSpec, HotKey, KbKey, LocalizedString, Menu, MenuItem, SysMods,
    WindowId,
};

use crate::app_state::AppState;
use crate::{cmd, CurrentAction, EditorState, SnippetId};

const SCRIBL_FILE_TYPE: FileSpec = FileSpec::new("Scribl animation (.scb)", &["scb"]);
const EXPORT_FILE_TYPE: FileSpec = FileSpec::new("mp4 video (.mp4)", &["mp4"]);

trait EditorMenu {
    fn action<F: FnMut(&mut MenuEventCtx, &mut EditorState) + 'static>(
        self,
        id: WindowId,
        f: F,
    ) -> Self;

    fn active_if<F: FnMut(&EditorState) -> bool + 'static>(self, id: WindowId, f: F) -> Self;
}

impl EditorMenu for MenuItem<AppState> {
    fn action<F: FnMut(&mut MenuEventCtx, &mut EditorState) + 'static>(
        self,
        id: WindowId,
        mut f: F,
    ) -> Self {
        self.on_activate(move |ctx, data, _env| {
            if let Some(editor_data) = data.editor_mut(id) {
                f(ctx, editor_data);
            } else {
                log::warn!("got a menu command for a nonexistent window");
            }
        })
    }

    fn active_if<F: FnMut(&EditorState) -> bool + 'static>(self, id: WindowId, mut f: F) -> Self {
        self.enabled_if(move |data, _env| {
            if let Some(editor_data) = data.editor(id) {
                f(editor_data)
            } else {
                log::warn!("checking enabled for a nonexistent window");
                false
            }
        })
    }
}

pub fn save_dialog_options() -> FileDialogOptions {
    FileDialogOptions::new().allowed_types(vec![SCRIBL_FILE_TYPE])
}

fn file_menu(window_id: WindowId, _data: &AppState) -> Menu<AppState> {
    let new = platform_menus::win::file::new();

    let open_cmd = commands::SHOW_OPEN_PANEL
        .with(FileDialogOptions::new().allowed_types(vec![SCRIBL_FILE_TYPE]));
    let open = MenuItem::new(LocalizedString::new("common-menu-file-open"))
        .command(open_cmd)
        .hotkey(SysMods::Cmd, "o");

    let save = MenuItem::new(LocalizedString::new("common-menu-file-save"))
        .action(window_id, move |ctx, data| {
            let save_as_command = commands::SHOW_SAVE_PANEL.with(save_dialog_options());
            if data.save_path.is_some() {
                ctx.submit_command(commands::SAVE_FILE)
            } else {
                ctx.submit_command(save_as_command)
            }
        })
        .hotkey(SysMods::Cmd, "s");

    let save_as = MenuItem::new(LocalizedString::new("common-menu-file-save-as"))
        .command(commands::SHOW_SAVE_PANEL.with(save_dialog_options()))
        .hotkey(SysMods::CmdShift, "S");

    let export =
        MenuItem::new(LocalizedString::new("scribl-menu-file-export").with_placeholder("Export"))
            .action(window_id, move |ctx, data| {
                let mut export_options = FileDialogOptions::new()
                    .allowed_types(vec![EXPORT_FILE_TYPE])
                    .title("Export to video")
                    .button_text("Export")
                    .accept_command(cmd::EXPORT_CURRENT);
                if let Some(save_path) = &data.save_path {
                    if let Some(save_name) = save_path.file_stem() {
                        if let Some(save_name) = save_name.to_str() {
                            export_options = export_options.default_name(save_name);
                        }
                    }
                }
                ctx.submit_command(commands::SHOW_SAVE_PANEL.with(export_options))
            })
            .hotkey(SysMods::Cmd, "e");

    let close = MenuItem::new(LocalizedString::new("common-menu-file-close"))
        .command(druid::commands::CLOSE_WINDOW)
        .hotkey(SysMods::Cmd, "q");

    Menu::new(LocalizedString::new("common-menu-file-menu"))
        .entry(new)
        .entry(open)
        .entry(save)
        .entry(save_as)
        .entry(export)
        .separator()
        .entry(close)
}

fn edit_menu(id: WindowId, _data: &AppState) -> Menu<AppState> {
    fn undo_desc(id: WindowId, data: &AppState) -> String {
        // FIXME: figure out how localization is expected to work
        let s = data
            .editor(id)
            .map(|e| format!("Undo {}", e.undo.borrow().undo_description().unwrap_or("")));
        s.unwrap_or(String::new())
    }

    fn redo_desc(id: WindowId, data: &AppState) -> String {
        // FIXME: figure out how localization is expected to work
        let s = data
            .editor(id)
            .map(|e| format!("Redo {}", e.undo.borrow().redo_description().unwrap_or("")));
        s.unwrap_or(String::new())
    }

    let undo = MenuItem::new(move |data: &AppState, _env: &Env| undo_desc(id, data))
        .command(commands::UNDO)
        .active_if(id, move |data| data.undo.borrow().can_undo())
        .hotkey(SysMods::Cmd, "z");

    let redo = MenuItem::new(move |data: &AppState, _env: &Env| redo_desc(id, data))
        .command(commands::REDO)
        .active_if(id, move |data| data.undo.borrow().can_redo())
        .hotkey(SysMods::CmdShift, "z");

    let draw =
        MenuItem::new(LocalizedString::new("scribl-menu-edit-draw").with_placeholder("Draw"))
            .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::DRAW))
            .active_if(id, move |data| !data.action.is_recording())
            .hotkey(SysMods::None, " ");

    let talk =
        MenuItem::new(LocalizedString::new("scribl-menu-edit-talk").with_placeholder("Talk"))
            .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::TALK))
            .active_if(id, move |data| !data.action.is_recording_audio())
            .hotkey(SysMods::Shift, " ");

    let play =
        MenuItem::new(LocalizedString::new("scribl-menu-edit-play").with_placeholder("Play"))
            .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::PLAY))
            .active_if(id, move |data| !data.action.is_playing())
            .hotkey(SysMods::None, KbKey::Enter);

    let stop =
        MenuItem::new(LocalizedString::new("scribl-menu-edit-stop").with_placeholder("Stop"))
            .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::STOP))
            .active_if(id, move |data| match data.action {
                CurrentAction::Playing => true,
                CurrentAction::Recording(_) => true,
                CurrentAction::RecordingAudio(_) => true,
                _ => false,
            })
            .dynamic_hotkey(move |data, _| {
                let action = data
                    .editor(id)
                    .map(|d| &d.action)
                    .unwrap_or(&CurrentAction::Idle);
                match action {
                    // The stop hotkey matches the hotkey that was used to start the current action.
                    CurrentAction::Playing => Some(HotKey::new(SysMods::None, KbKey::Enter)),
                    CurrentAction::Recording(_) => Some(HotKey::new(SysMods::None, " ")),
                    CurrentAction::RecordingAudio(_) => Some(HotKey::new(SysMods::Shift, " ")),
                    _ => None,
                }
            });

    let mark =
        MenuItem::new(LocalizedString::new("scribl-menu-edit-mark").with_placeholder("Set mark"))
            .action(id, move |_, data| data.set_mark())
            .hotkey(SysMods::None, "m");

    let clear_mark = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-clear-mark").with_placeholder("Clear mark"),
    )
    .action(id, move |_, data| data.clear_mark())
    .hotkey(SysMods::None, KbKey::Escape)
    .active_if(id, move |data| data.mark.is_some());

    let warp = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-warp").with_placeholder("Warp snippet"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::LERP_SNIPPET))
    .hotkey(SysMods::None, "w")
    .active_if(id, move |data| {
        data.mark.is_some() && matches!(data.selected_snippet, Some(SnippetId::Draw(_)))
    });

    let trunc = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-truncate").with_placeholder("Truncate snippet"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::TRUNCATE_SNIPPET))
    .hotkey(SysMods::None, "t")
    .active_if(id, move |data| data.selected_snippet.is_some());

    let delete = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-delete").with_placeholder("Delete selected"),
    )
    .action(id, move |_, data| data.delete_selected_snippet())
    .hotkey(SysMods::None, KbKey::Delete)
    .active_if(id, move |data| data.selected_snippet.is_some());

    let talk_selected =
        move |data: &EditorState| matches!(data.selected_snippet, Some(SnippetId::Talk(_)));

    let increase_volume = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-increase-volume")
            .with_placeholder("Increase volume"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::MULTIPLY_VOLUME.with(1.1)))
    .hotkey(SysMods::None, "+")
    .active_if(id, talk_selected);

    let decrease_volume = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-decrease-volume")
            .with_placeholder("Decrease volume"),
    )
    .on_activate(|ctx, _: &mut AppState, _| {
        ctx.submit_command(cmd::MULTIPLY_VOLUME.with(1.0 / 1.1))
    })
    .hotkey(SysMods::None, "-")
    .active_if(id, talk_selected);

    let silence = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-silence").with_placeholder("Silence selected audio"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::SILENCE_AUDIO))
    .hotkey(SysMods::None, KbKey::Backspace)
    .active_if(id, talk_selected);

    let snip = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-snip").with_placeholder("Snip selected audio"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::SNIP_AUDIO))
    .hotkey(SysMods::Shift, KbKey::Backspace)
    .active_if(id, talk_selected);

    Menu::new(LocalizedString::new("common-menu-edit-menu"))
        .entry(undo)
        .entry(redo)
        .separator()
        .entry(draw)
        .entry(talk)
        .entry(play)
        .entry(stop)
        .separator()
        .entry(mark)
        .entry(clear_mark)
        .entry(warp)
        .entry(trunc)
        .entry(delete)
        .separator()
        .entry(increase_volume)
        .entry(decrease_volume)
        .entry(silence)
        .entry(snip)
}

fn view_menu(id: WindowId, _data: &AppState) -> Menu<AppState> {
    let zoom_in =
        MenuItem::new(LocalizedString::new("scribl-menu-view-zoom-in").with_placeholder("Zoom in"))
            .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::ZOOM_IN))
            .active_if(id, move |data| data.settings.can_zoom_in());

    let zoom_out = MenuItem::new(
        LocalizedString::new("scribl-menu-view-zoom-out").with_placeholder("Zoom out"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::ZOOM_OUT))
    .active_if(id, move |data| data.settings.can_zoom_out());

    let zoom_reset = MenuItem::new(
        LocalizedString::new("scribl-menu-view-zoom-reset").with_placeholder("Reset zoom"),
    )
    .on_activate(|ctx, _: &mut AppState, _| ctx.submit_command(cmd::ZOOM_RESET));

    Menu::new(LocalizedString::new("scribl-menu-view-menu").with_placeholder("View"))
        .entry(zoom_in)
        .entry(zoom_out)
        .entry(zoom_reset)
}

pub fn make_menu(window_id: Option<WindowId>, data: &AppState) -> Menu<AppState> {
    // FIXME: do something sane if there's no window id
    if let Some(id) = window_id {
        Menu::empty()
            .entry(file_menu(id, data))
            .entry(edit_menu(id, data))
            .entry(view_menu(id, data))
    } else {
        Menu::empty()
    }
}
