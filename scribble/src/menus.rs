use druid::commands;
use druid::platform_menus;
use druid::{
    Command, FileDialogOptions, FileSpec, KeyCode, LocalizedString, MenuDesc, MenuItem, SysMods,
};

use crate::cmd;
use crate::data::CurrentAction;
use crate::widgets::ToggleButtonState;

const SCRIBBLE_FILE_TYPE: FileSpec = FileSpec::new("Scribble animation", &["scb"]);
const EXPORT_FILE_TYPE: FileSpec = FileSpec::new("mp4 video", &["mp4"]);

use crate::data::AppState;

fn file_menu(data: &AppState) -> MenuDesc<AppState> {
    let has_path = data.save_path.is_some();

    let open = MenuItem::new(
        LocalizedString::new("common-menu-file-open"),
        Command::new(
            commands::SHOW_OPEN_PANEL,
            FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE]),
        ),
    )
    .hotkey(SysMods::Cmd, "o");

    let save_as_command = Command::new(
        commands::SHOW_SAVE_PANEL,
        FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE]),
    );
    let save_command = if has_path {
        commands::SAVE_FILE.into()
    } else {
        save_as_command.clone()
    };
    let save = MenuItem::new(LocalizedString::new("common-menu-file-save"), save_command)
        .hotkey(SysMods::Cmd, "s");

    let save_as = MenuItem::new(
        LocalizedString::new("common-menu-file-save-as"),
        save_as_command,
    );

    // Note that we're reusing the SHOW_SAVE_PANEL command for exporting. There doesn't appear to
    // be another way to get the system file dialog.
    let export = MenuItem::new(
        LocalizedString::new("scribble-menu-file-export").with_placeholder("Export"),
        Command::new(
            commands::SHOW_SAVE_PANEL,
            FileDialogOptions::new().allowed_types(vec![EXPORT_FILE_TYPE]),
        ),
    )
    .hotkey(SysMods::Cmd, "e");

    MenuDesc::new(LocalizedString::new("common-menu-file-menu"))
        .append(open)
        .append(save)
        .append(save_as)
        .append(export)
        .append_separator()
        .append(platform_menus::win::file::exit())
}

fn edit_menu(data: &AppState) -> MenuDesc<AppState> {
    let undo = platform_menus::common::undo().disabled_if(|| !data.undo.borrow().can_undo());
    let redo = platform_menus::common::redo().disabled_if(|| !data.undo.borrow().can_redo());

    let draw = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-draw").with_placeholder("Draw"),
        cmd::DRAW,
    )
    .hotkey(SysMods::Cmd, "d")
    .disabled_if(|| data.action.rec_toggle() != ToggleButtonState::ToggledOff);

    let talk = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-talk").with_placeholder("Talk"),
        cmd::TALK,
    )
    .hotkey(SysMods::Cmd, "t")
    .disabled_if(|| data.action.rec_audio_toggle() != ToggleButtonState::ToggledOff);

    let play = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-play").with_placeholder("Play"),
        cmd::PLAY,
    )
    .hotkey(SysMods::Cmd, "p")
    .disabled_if(|| data.action.play_toggle() != ToggleButtonState::ToggledOff);

    let stop = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-stop").with_placeholder("Stop"),
        cmd::STOP,
    )
    .hotkey(SysMods::None, KeyCode::Space)
    .disabled_if(|| {
        !matches!(data.action,
            CurrentAction::Playing
            | CurrentAction::Recording(_)
            | CurrentAction::WaitingToRecord(_)
            | CurrentAction::RecordingAudio(_))
    });

    let mark = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-mark").with_placeholder("Set mark"),
        cmd::SET_MARK,
    )
    .hotkey(SysMods::None, KeyCode::KeyM);

    let warp = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-warp").with_placeholder("Warp snippet"),
        cmd::LERP_SNIPPET,
    )
    .hotkey(SysMods::None, KeyCode::KeyW)
    .disabled_if(|| data.scribble.mark.is_none());

    let trunc = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-truncate").with_placeholder("Truncate snippet"),
        cmd::TRUNCATE_SNIPPET,
    )
    .hotkey(SysMods::None, KeyCode::KeyT)
    .disabled_if(|| data.scribble.selected_snippet.is_none());

    let delete = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-delete").with_placeholder("Delete selected"),
        cmd::DELETE_SNIPPET,
    )
    .hotkey(SysMods::None, KeyCode::Delete)
    .disabled_if(|| data.scribble.selected_snippet.is_none());

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

pub fn make_menu(data: &AppState) -> MenuDesc<AppState> {
    MenuDesc::empty()
        .append(file_menu(data))
        .append(edit_menu(data))
}
