#![allow(dead_code)]

use druid::theme;

use druid::{AppLauncher, Color, Key, LocalizedString, WindowDesc};
use std::time::Duration;

mod app_delegate;
mod audio;
mod cmd;
mod data;
mod encode;
mod menus;
mod snippet_layout;
mod undo;
mod widgets;

const BUTTON_BACKGROUND_DISABLED: Key<Color> = Key::new("button_background_disabled");
const BUTTON_FOREGROUND_DISABLED: Key<Color> = Key::new("button_foreground_disabled");
const BUTTON_ICON_PADDING: Key<f64> = Key::new("scribble.button_icon_padding");
const RADIO_BUTTON_ICON_SELECTED: Key<Color> = Key::new("scribble-radio-button-icon-selected");
const RADIO_BUTTON_ICON_HOT: Key<Color> = Key::new("scribble-radio-button-icon-hot");
pub const FRAME_TIME: Duration = Duration::from_millis(16);
pub const TEXT_SIZE_SMALL: Key<f64> = Key::new("text_size_small");

use data::AppState;
use widgets::Root;

fn main() {
    if let Err(e) = gstreamer::init() {
        println!("failed to init gstreamer: {}", e);
        return;
    }

    let initial_state = AppState::default();
    let scribble_state = initial_state.scribble.clone();

    let main_window = WindowDesc::new(|| Root::new(scribble_state))
        .title(LocalizedString::new("Scribble"))
        .menu(menus::make_menu(&initial_state))
        .window_size((400.0, 400.0));

    AppLauncher::with_window(main_window)
        .delegate(app_delegate::Delegate::default())
        .configure_env(|e, _| {
            e.set(theme::BUTTON_LIGHT, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_BACKGROUND_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
            e.set(BUTTON_FOREGROUND_DISABLED, Color::rgb8(0x33, 0x33, 0x33));
            e.set(RADIO_BUTTON_ICON_SELECTED, Color::rgb8(45, 214, 51));
            e.set(RADIO_BUTTON_ICON_HOT, Color::rgb8(197, 237, 198));
            e.set(BUTTON_ICON_PADDING, 2.0);
            e.set(TEXT_SIZE_SMALL, 10.0);
        })
        .launch(initial_state)
        .expect("failed to launch");
}
