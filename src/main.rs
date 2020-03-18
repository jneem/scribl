#![allow(dead_code)]

use druid::{AppLauncher, Color, LensExt, Key, LocalizedString, Widget, WindowDesc};
use druid::widget::{Align, Flex, WidgetExt};
use druid::theme;

mod consts;
mod data;
mod lerp;
mod snippet;
mod widgets;

const BUTTON_DISABLED: Key<Color> = Key::new("button_disabled");

use data::{ScribbleState, CurrentAction};
use widgets::{DrawingPane, ToggleButton, ToggleButtonState};

fn build_root_widget() -> impl Widget<ScribbleState> {
    let drawing = DrawingPane::default();
    let rec_button: ToggleButton<ToggleButtonState> = ToggleButton::new(
        "Rec",
        |_, _, _| {dbg!("On");},
        |_, _, _| {dbg!("Off");}
    );
    let rec_button_lens = rec_button.lens(ScribbleState::action.map(
        CurrentAction::get_rec_toggle,
        CurrentAction::put_rec_toggle
    ));
    let button_row = Flex::row()
        .with_child(rec_button_lens, 0.0);
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
        .configure_env( |e, _| {
            e.set(theme::BUTTON_LIGHT, Color::rgb8(0x70, 0x70, 0x70));
            e.set(BUTTON_DISABLED, Color::rgb8(0x55, 0x55, 0x55));
        })
        .launch(initial_state)
        .expect("failed to launch");
}
