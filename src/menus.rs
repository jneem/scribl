use druid::commands;
use druid::platform_menus;
use druid::{
    LocalizedString,
    MenuDesc,
    FileDialogOptions,
    FileSpec,
    MenuItem,
    Command,
};

const SCRIBBLE_FILE_TYPE: FileSpec = FileSpec::new("Scribble animation", &["scb"]);

use crate::data::ScribbleState;

fn create_file_menu(data: &ScribbleState) -> MenuDesc<ScribbleState> {
    let has_path = data.save_path.is_some();
    dbg!(has_path);

    let open = MenuItem::new(
        LocalizedString::new("common-menu-file-open"),
        Command::new(
            commands::SHOW_OPEN_PANEL,
            FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE])
        )
    );

    let save_as = MenuItem::new(
        LocalizedString::new("common-menu-file-save-as"),
        Command::new(
            commands::SHOW_SAVE_PANEL,
            FileDialogOptions::new().allowed_types(vec![SCRIBBLE_FILE_TYPE])
        )
    );

    let mut menu = MenuDesc::new(LocalizedString::new("common-menu-file-menu"))
        .append(open);

    if has_path {
        menu = menu.append(platform_menus::win::file::save());
    }

    menu.append(save_as)
        .append(platform_menus::win::file::exit())
}

pub fn make_menu(data: &ScribbleState) -> MenuDesc<ScribbleState> {
    MenuDesc::empty()
        .append(create_file_menu(data))
}
