#![allow(dead_code)]

use druid::theme;
use druid::widget::{Align, Flex, WidgetExt};
use druid::{AppLauncher, Color, Env, EventCtx, Key, LensExt, LocalizedString, Widget, WindowDesc};
use std::time::{Duration, Instant};

mod consts;
mod data;
mod lerp;
mod snippet;
mod widgets;

const BUTTON_DISABLED: Key<Color> = Key::new("button_disabled");
pub const FRAME_TIME: Duration = Duration::from_millis(16);

use data::{CurrentAction, ScribbleState};
use widgets::{DrawingPane, ToggleButton, ToggleButtonState};

fn rec_button_on(ctxt: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_recording();
}

fn rec_button_off(_ctxt: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    dbg!("Stopped recording", data.time_us);
    data.stop_recording();
}

fn play_button_on(ctxt: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_playing();
}

fn play_button_off(_ctxt: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.stop_playing();
}

fn build_root_widget() -> impl Widget<ScribbleState> {
    let drawing = DrawingPane::default();
    let rec_button: ToggleButton<ScribbleState> = ToggleButton::new(
        "Rec",
        |state: &ScribbleState| state.action.rec_toggle(),
        &rec_button_on,
        &rec_button_off,
    );
    let play_button = ToggleButton::new(
        "Play",
        |state: &ScribbleState| state.action.play_toggle(),
        &play_button_on,
        &play_button_off,
    );

    let button_row = Flex::row()
        .with_child(rec_button, 0.0)
        .with_child(play_button, 0.0);
    let column = Flex::column()
        .with_child(button_row, 0.0)
        .with_child(drawing, 0.0);

    Align::centered(column)
}

fn main() {
    let main_window = WindowDesc::new(build_root_widget)
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
