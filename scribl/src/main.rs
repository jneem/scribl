use clap::{App, Arg};
use druid::theme;
use druid::{AppLauncher, Color, Key};
use std::time::Duration;

mod app_delegate;
mod app_state;
mod audio;
mod autosave;
mod cmd;
mod editor_state;
mod encode;
mod imagebuf;
mod menus;
mod save_state;
mod snippet_layout;
mod undo;
mod widgets;

const BUTTON_BACKGROUND_DISABLED: Key<Color> = Key::new("button_background_disabled");
const BUTTON_FOREGROUND_DISABLED: Key<Color> = Key::new("button_foreground_disabled");
const BUTTON_ICON_PADDING: Key<f64> = Key::new("scribl.button_icon_padding");
const BUTTON_ICON_DISABLED: Key<Color> = Key::new("scribl-radio-button-icon-disabled");
const BUTTON_ICON_SELECTED: Key<Color> = Key::new("scribl-radio-button-icon-selected");
const BUTTON_ICON_HOT: Key<Color> = Key::new("scribl-radio-button-icon-hot");
const BUTTON_ICON_IDLE: Key<Color> = Key::new("scribl-radio-button-icon-idle");
const BUTTON_GROUP_BORDER_WIDTH: Key<f64> = Key::new("scribl-button-group-border-width");
pub const FRAME_TIME: Duration = Duration::from_millis(16);
pub const TEXT_SIZE_SMALL: Key<f64> = Key::new("scribl-text-size-small");
pub const FONT_NAME_MONO: Key<&str> = Key::new("scribl-font-name-mono");

use app_state::AppState;
use editor_state::EditorState;

const MAJOR: u32 = pkg_version::pkg_version_major!();
const MINOR: u32 = pkg_version::pkg_version_minor!();
const PATCH: u32 = pkg_version::pkg_version_patch!();

// These colors are lightened versions of the utexas secondary color palette. We use them
// for coloring the UI elements.
pub const UI_LIGHT_YELLOW: Color = Color::rgb8(255, 239, 153);
pub const UI_LIGHT_GREEN: Color = Color::rgb8(211, 230, 172);
pub const UI_DARK_GREEN: Color = Color::rgb8(87, 157, 66);
pub const UI_LIGHT_BLUE: Color = Color::rgb8(87, 242, 255);
pub const UI_DARK_BLUE: Color = Color::rgb8(0, 95, 134);
pub const UI_BEIGE: Color = Color::rgb8(214, 210, 196);
pub const UI_LIGHT_STEEL_BLUE: Color = Color::rgb8(156, 173, 183);

fn main() {
    env_logger::init();

    if let Err(e) = gstreamer::init() {
        log::error!("failed to init gstreamer: {}", e);
        return;
    }

    let matches = App::new("scribl")
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
    let editor_window_id = editor_window_desc.id;

    let launcher = AppLauncher::with_window(editor_window_desc)
        .delegate(app_delegate::Delegate::default())
        .configure_env(|e, _| {
            e.set(theme::BUTTON_LIGHT, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_BACKGROUND_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
            e.set(BUTTON_FOREGROUND_DISABLED, Color::rgb8(0x33, 0x33, 0x33));
            e.set(BUTTON_ICON_DISABLED, Color::rgb8(0x33, 0x33, 0x33));
            e.set(BUTTON_ICON_SELECTED, UI_DARK_GREEN);
            e.set(BUTTON_ICON_HOT, UI_LIGHT_GREEN);
            e.set(BUTTON_ICON_IDLE, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_ICON_PADDING, 2.0);
            e.set(BUTTON_GROUP_BORDER_WIDTH, 1.0);
            e.set(TEXT_SIZE_SMALL, 10.0);
            e.set(FONT_NAME_MONO, "monospace");
        });

    let ext_handle = launcher.get_external_handle();
    if let Err(e) = ext_handle.submit_command(
        crate::cmd::INITIALIZE_EVENT_SINK,
        ext_handle.clone(),
        editor_window_id,
    ) {
        log::error!(
            "failed to initialize event sink, loading files won't work: {}",
            e
        );
    }

    launcher.launch(initial_state).expect("failed to launch");
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
        use crate::editor_state::StatusMsg;
        use crate::encode::EncodingStatus;
        match msg {
            StatusMsg::Encoding(e) => match e {
                // TODO: nicer display
                EncodingStatus::Encoding(pct) => eprintln!("{}", pct),
                EncodingStatus::Error(s) => eprintln!("Encoding error: {}", s),
                EncodingStatus::Finished(_) => eprintln!("Finished!"),
            },
            _ => panic!("unexpected status message!"),
        }
    }
}
