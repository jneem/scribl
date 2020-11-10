use druid::commands;
use druid::platform_menus;
use druid::{FileDialogOptions, FileSpec, KbKey, LocalizedString, MenuDesc, MenuItem, SysMods};

use scribl_widget::ToggleButtonState;

use crate::app_state::AppState;
use crate::cmd;
use crate::editor_state::{CurrentAction, EditorState};

const SCRIBL_FILE_TYPE: FileSpec = FileSpec::new("Scribl animation (.scb)", &["scb"]);
const EXPORT_FILE_TYPE: FileSpec = FileSpec::new("mp4 video (.mp4)", &["mp4"]);

pub fn save_dialog_options() -> FileDialogOptions {
    FileDialogOptions::new().allowed_types(vec![SCRIBL_FILE_TYPE])
}

fn file_menu(data: &EditorState) -> MenuDesc<AppState> {
    let has_path = data.save_path.is_some();

    let new = platform_menus::win::file::new();

    let open = MenuItem::new(
        LocalizedString::new("common-menu-file-open"),
        commands::SHOW_OPEN_PANEL
            .with(FileDialogOptions::new().allowed_types(vec![SCRIBL_FILE_TYPE])),
    )
    .hotkey(SysMods::Cmd, "o");

    let save_as_command = commands::SHOW_SAVE_PANEL.with(save_dialog_options());
    let save_command = if has_path {
        commands::SAVE_FILE.with(None)
    } else {
        save_as_command.clone()
    };
    let save = MenuItem::new(LocalizedString::new("common-menu-file-save"), save_command)
        .hotkey(SysMods::Cmd, "s");

    let save_as = MenuItem::new(
        LocalizedString::new("common-menu-file-save-as"),
        save_as_command,
    )
    .hotkey(SysMods::CmdShift, "S");

    // Note that we're reusing the SHOW_SAVE_PANEL command for exporting. There doesn't appear to
    // be another way to get the system file dialog.
    let export = MenuItem::new(
        LocalizedString::new("scribl-menu-file-export").with_placeholder("Export"),
        commands::SHOW_SAVE_PANEL
            .with(FileDialogOptions::new().allowed_types(vec![EXPORT_FILE_TYPE])),
    )
    .hotkey(SysMods::Cmd, "e");

    let close = MenuItem::new(
        LocalizedString::new("common-menu-file-close"),
        druid::commands::CLOSE_WINDOW,
    )
    .hotkey(SysMods::Cmd, "q");

    MenuDesc::new(LocalizedString::new("common-menu-file-menu"))
        .append(new)
        .append(open)
        .append(save)
        .append(save_as)
        .append(export)
        .append_separator()
        .append(close)
}

fn edit_menu(data: &EditorState) -> MenuDesc<AppState> {
    let undo = if data.undo.borrow().can_undo() {
        MenuItem::new(
            // FIXME: figure out how localization is expected to work
            LocalizedString::new("scribl-menu-edit-undo").with_placeholder(format!(
                "Undo {}",
                data.undo.borrow().undo_description().unwrap_or("")
            )),
            commands::UNDO,
        )
        .hotkey(SysMods::Cmd, "z")
    } else {
        platform_menus::common::undo().disabled()
    };

    let redo = if data.undo.borrow().can_redo() {
        MenuItem::new(
            LocalizedString::new("scribl-menu-edit-redo").with_placeholder(format!(
                "Redo {}",
                data.undo.borrow().redo_description().unwrap_or("")
            )),
            commands::REDO,
        )
        .hotkey(SysMods::CmdShift, "z")
    } else {
        platform_menus::common::redo().disabled()
    };

    let draw = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-draw").with_placeholder("Draw"),
        cmd::DRAW,
    );
    let draw = if data.action.rec_toggle() == ToggleButtonState::ToggledOff {
        draw.hotkey(SysMods::None, " ")
    } else {
        draw.disabled()
    };

    let talk = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-talk").with_placeholder("Talk"),
        cmd::TALK,
    );
    let talk = if data.action.rec_audio_toggle() == ToggleButtonState::ToggledOff {
        talk.hotkey(SysMods::Shift, " ")
    } else {
        talk.disabled()
    };

    let play = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-play").with_placeholder("Play"),
        cmd::PLAY,
    );
    let play = if data.action.play_toggle() == ToggleButtonState::ToggledOff {
        play.hotkey(SysMods::None, KbKey::Enter)
    } else {
        play.disabled()
    };

    let stop = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-stop").with_placeholder("Stop"),
        cmd::STOP,
    );
    // The stop hotkey matches the hotkey that was used to start the current action.
    let stop = match data.action {
        CurrentAction::Playing => stop.hotkey(SysMods::None, KbKey::Enter),
        CurrentAction::Recording(_) => stop.hotkey(SysMods::None, " "),
        CurrentAction::RecordingAudio(_) => stop.hotkey(SysMods::Shift, " "),
        _ => stop.disabled(),
    };

    let mark = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-mark").with_placeholder("Set mark"),
        cmd::SET_MARK.with(None),
    )
    .hotkey(SysMods::Cmd, "m");

    let warp = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-warp").with_placeholder("Warp snippet"),
        cmd::LERP_SNIPPET,
    )
    .hotkey(SysMods::Cmd, "w")
    .disabled_if(|| data.mark.is_none());

    let trunc = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-truncate").with_placeholder("Truncate snippet"),
        cmd::TRUNCATE_SNIPPET,
    )
    .hotkey(SysMods::Cmd, "t")
    .disabled_if(|| data.selected_snippet.is_none());

    let delete = MenuItem::new(
        LocalizedString::new("scribl-menu-edit-delete").with_placeholder("Delete selected"),
        cmd::DELETE_SNIPPET,
    )
    .hotkey(SysMods::None, KbKey::Delete)
    .disabled_if(|| data.selected_snippet.is_none());

    MenuDesc::new(LocalizedString::new("common-menu-edit-menu"))
        .append(undo)
        .append(redo)
        .append_separator()
        .append(draw)
        .append(talk)
        .append(play)
        .append(stop)
        .append_separator()
        .append(mark)
        .append(warp)
        .append(trunc)
        .append(delete)
}

fn view_menu(data: &EditorState) -> MenuDesc<AppState> {
    let zoom_in = MenuItem::new(
        LocalizedString::new("scribl-menu-view-zoom-in").with_placeholder("Zoom in"),
        cmd::ZOOM_IN,
    )
    .disabled_if(|| data.zoom >= crate::editor_state::MAX_ZOOM);

    let zoom_out = MenuItem::new(
        LocalizedString::new("scribl-menu-view-zoom-out").with_placeholder("Zoom out"),
        cmd::ZOOM_OUT,
    )
    .disabled_if(|| data.zoom <= 1.0);

    let zoom_reset = MenuItem::new(
        LocalizedString::new("scribl-menu-view-zoom-reset").with_placeholder("Reset zoom"),
        cmd::ZOOM_RESET,
    );

    MenuDesc::new(LocalizedString::new("scribl-menu-view-menu").with_placeholder("View"))
        .append(zoom_in)
        .append(zoom_out)
        .append(zoom_reset)
}

pub fn make_menu(data: &EditorState) -> MenuDesc<AppState> {
    MenuDesc::empty()
        .append(file_menu(data))
        .append(edit_menu(data))
        .append(view_menu(data))
}
