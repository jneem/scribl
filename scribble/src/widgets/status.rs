use druid::lens;
use druid::widget::prelude::*;
use druid::widget::{Align, Either, Flex, Label, ProgressBar, WidgetExt};
use druid::LensExt;

use crate::data::AppState;
use crate::encode::EncodingStatus;

pub fn make_status_bar() -> impl Widget<AppState> {
    let time_label = Label::new(|data: &AppState, _env: &Env| {
        let usecs = data.time().as_micros();
        let mins = usecs / 60_000_000;
        let secs = (usecs / 1_000_000) % 60;
        let cents = (usecs / 10_000) % 100;
        format!("{:02}:{:02}.{:02}", mins, secs, cents)
    });

    let status_label_not_encoding =
        Label::new(|data: &Option<EncodingStatus>, _env: &Env| match data {
            None => String::new(),
            Some(EncodingStatus::Error(_)) => "Encoding failed!".to_owned(),
            Some(EncodingStatus::Encoding(_)) => unreachable!(),
            Some(EncodingStatus::Finished) => "Encoding finished".to_owned(),
        });

    let status_label_encoding =
        Label::new(|_data: &Option<EncodingStatus>, _env: &Env| "Encoding: ".to_owned());

    let progress = ProgressBar::new().lens(lens::Id.map(
        |s| {
            if let Some(EncodingStatus::Encoding(x)) = s {
                *x
            } else {
                0.0
            }
        },
        |_, _| {},
    ));

    let status_label = Either::new(
        |data: &Option<EncodingStatus>, _env| matches!(data, Some(EncodingStatus::Encoding(_))),
        Flex::row()
            .with_child(status_label_encoding)
            .with_child(progress)
            .with_flex_spacer(1.0),
        status_label_not_encoding,
    )
    .fix_width(250.0); // TODO: can we make this depend on the text width?

    let row = Flex::row()
        .with_child(time_label)
        .with_flex_spacer(1.0)
        .with_child(status_label.lens(AppState::encoding_status));
    Align::centered(row)
}
