use druid::commands;
use druid::platform_menus;
use druid::{
    Command, FileDialogOptions, FileSpec, KeyCode, LocalizedString, MenuDesc, MenuItem, SysMods,
};

use crate::cmd;

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
    );

    let save = if has_path {
        platform_menus::win::file::save()
    } else {
        platform_menus::win::file::save().disabled()
    };

    let save_as = MenuItem::new(
        LocalizedString::new("common-menu-file-save-as"),
        Command::new(
            commands::SHOW_SAVE_PANEL,
            FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE]),
        ),
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

fn edit_menu(_data: &AppState) -> MenuDesc<AppState> {
    // TODO: make these active/inactive depending on the current undo stack.
    let undo = platform_menus::common::undo();
    let redo = platform_menus::common::redo();

    let draw = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-draw").with_placeholder("Draw"),
        cmd::DRAW,
    )
    .hotkey(SysMods::Cmd, "d");

    let talk = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-talk").with_placeholder("Talk"),
        cmd::TALK,
    )
    .hotkey(SysMods::Cmd, "t");

    let play = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-play").with_placeholder("Play"),
        cmd::PLAY,
    )
    .hotkey(SysMods::Cmd, "p");

    let stop = MenuItem::new(
        LocalizedString::new("scribble-menu-edit-stop").with_placeholder("Stop"),
        cmd::STOP,
    )
    .hotkey(SysMods::None, KeyCode::Escape);

    MenuDesc::new(LocalizedString::new("common-menu-edit-menu"))
        .append(undo)
        .append(redo)
        .append_separator()
        .append(draw)
        .append(talk)
        .append(play)
        .append(stop)
}

pub fn make_menu(data: &AppState) -> MenuDesc<AppState> {
    MenuDesc::empty()
        .append(file_menu(data))
        .append(edit_menu(data))
}
