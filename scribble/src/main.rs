use clap::{App, Arg};
use druid::theme;
use druid::{AppLauncher, Color, Key};
use std::time::Duration;

mod app_delegate;
mod app_state;
mod audio;
mod cmd;
mod editor_state;
mod encode;
mod menus;
mod save_state;
mod snippet_layout;
mod undo;
mod widgets;

const BUTTON_BACKGROUND_DISABLED: Key<Color> = Key::new("button_background_disabled");
const BUTTON_FOREGROUND_DISABLED: Key<Color> = Key::new("button_foreground_disabled");
const BUTTON_ICON_PADDING: Key<f64> = Key::new("scribble.button_icon_padding");
const BUTTON_ICON_DISABLED: Key<Color> = Key::new("scribble-radio-button-icon-disabled");
const BUTTON_ICON_SELECTED: Key<Color> = Key::new("scribble-radio-button-icon-selected");
const BUTTON_ICON_HOT: Key<Color> = Key::new("scribble-radio-button-icon-hot");
const BUTTON_ICON_IDLE: Key<Color> = Key::new("scribble-radio-button-icon-idle");
pub const FRAME_TIME: Duration = Duration::from_millis(16);
pub const TEXT_SIZE_SMALL: Key<f64> = Key::new("text_size_small");

use app_state::AppState;
use editor_state::EditorState;

const MAJOR: u32 = pkg_version::pkg_version_major!();
const MINOR: u32 = pkg_version::pkg_version_minor!();
const PATCH: u32 = pkg_version::pkg_version_patch!();

fn main() {
    env_logger::init();

    if let Err(e) = gstreamer::init() {
        log::error!("failed to init gstreamer: {}", e);
        return;
    }

    let matches = App::new("scribble")
        .version(format!("{}.{}.{}", MAJOR, MINOR, PATCH).as_str())
        .author("Joe Neeman <joeneeman@gmail.com>")
        .arg(
            Arg::with_name("FILE")
                .help("The file to open")
                .takes_value(true),
        )
        .arg(
            Arg::with_name("export-to")
                .help("Export the animation as a video instead of opening it")
                .long("export-to")
                .takes_value(true),
        )
        .get_matches();

    let initial_editor = if let Some(path) = matches.value_of("FILE") {
        match crate::save_state::SaveFileData::load_from_path(path) {
            Ok(save_file) => EditorState::from_save_file(save_file),
            Err(e) => {
                log::error!("Error opening save file: {}", e);
                return;
            }
        }
    } else {
        EditorState::default()
    };

    if let Some(output_path) = matches.value_of("export-to") {
        encode(initial_editor, output_path);
        return;
    }

    let mut initial_state = AppState::default();
    let editor_window_desc = initial_state.add_editor(initial_editor);

    AppLauncher::with_window(editor_window_desc)
        .delegate(app_delegate::Delegate::default())
        .configure_env(|e, _| {
            e.set(theme::BUTTON_LIGHT, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_BACKGROUND_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
            e.set(BUTTON_FOREGROUND_DISABLED, Color::rgb8(0x33, 0x33, 0x33));
            e.set(BUTTON_ICON_DISABLED, Color::rgb8(0x33, 0x33, 0x33));
            e.set(BUTTON_ICON_SELECTED, Color::rgb8(45, 214, 51));
            e.set(BUTTON_ICON_HOT, Color::rgb8(197, 237, 198));
            e.set(BUTTON_ICON_IDLE, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_ICON_PADDING, 2.0);
            e.set(TEXT_SIZE_SMALL, 10.0);
        })
        .launch(initial_state)
        .expect("failed to launch");
}

fn encode(data: EditorState, path: &str) {
    let export = cmd::ExportCmd {
        snippets: data.snippets,
        audio_snippets: data.audio_snippets,
        filename: path.into(),
    };
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || crate::encode::encode_blocking(export, tx));

    for msg in rx.iter() {
        use crate::encode::EncodingStatus;
        match msg {
            // TODO: nicer display
            EncodingStatus::Encoding(pct) => eprintln!("{}", pct),
            EncodingStatus::Error(s) => eprintln!("Encoding error: {}", s),
            EncodingStatus::Finished => eprintln!("Finished!"),
        }
    }
}
