#![allow(dead_code)]

use druid::theme;

use druid::{AppLauncher, Color, Key, LocalizedString, WindowDesc};
use std::time::Duration;

mod consts;
mod data;
mod lerp;
mod snippet;
mod widgets;

const BUTTON_DISABLED: Key<Color> = Key::new("button_disabled");
pub const FRAME_TIME: Duration = Duration::from_millis(16);

use data::ScribbleState;
use widgets::Root;

fn main() {
    let main_window = WindowDesc::new(Root::new)
        .title(LocalizedString::new("Hello!"))
        .window_size((400.0, 400.0));

    let initial_state = ScribbleState::default();
    AppLauncher::with_window(main_window)
        .configure_env(|e, _| {
            e.set(theme::BUTTON_LIGHT, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
        })
        .launch(initial_state)
        .expect("failed to launch");
}
