#![allow(dead_code)]

use druid::theme;

use druid::{AppLauncher, Color, Key, LocalizedString, WindowDesc};
use std::time::Duration;

mod app_delegate;
mod audio;
mod consts;
mod data;
mod lerp;
mod menus;
mod snippet;
mod snippet_layout;
mod undo;
mod widgets;

const BUTTON_DISABLED: Key<Color> = Key::new("button_disabled");
pub const FRAME_TIME: Duration = Duration::from_millis(16);

use data::AppState;
use widgets::Root;

fn main() {
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
            e.set(BUTTON_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
        })
        .launch(initial_state)
        .expect("failed to launch");
}
