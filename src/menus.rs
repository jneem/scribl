use druid::commands;
use druid::platform_menus;
use druid::{Command, FileDialogOptions, FileSpec, LocalizedString, MenuDesc, MenuItem};

const SCRIBBLE_FILE_TYPE: FileSpec = FileSpec::new("Scribble animation", &["scb"]);

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

    let save_as = MenuItem::new(
        LocalizedString::new("common-menu-file-save-as"),
        Command::new(
            commands::SHOW_SAVE_PANEL,
            FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE]),
        ),
    );

    let mut menu = MenuDesc::new(LocalizedString::new("common-menu-file-menu")).append(open);

    if has_path {
        menu = menu.append(platform_menus::win::file::save());
    }

    menu.append(save_as)
        .append(platform_menus::win::file::exit())
}

fn edit_menu(_data: &AppState) -> MenuDesc<AppState> {
    // TODO: make these active/inactive depending on the current undo stack.
    let undo = platform_menus::common::undo();
    let redo = platform_menus::common::redo();

    MenuDesc::new(LocalizedString::new("common-menu-edit-menu"))
        .append(undo)
        .append(redo)
}

pub fn make_menu(data: &AppState) -> MenuDesc<AppState> {
    MenuDesc::empty()
        .append(file_menu(data))
        .append(edit_menu(data))
}
