use druid::lens;
use druid::widget::prelude::*;
use druid::widget::{Align, Either, Flex, Label, ProgressBar, WidgetExt};
use druid::{Data, LensExt};
use std::borrow::Cow;
use std::path::Path;

use crate::editor_state::{AsyncOpsStatus, EditorState, FinishedStatus};

// We have two possible status widgets: one is just a label; the other is a label + progress bar.
#[derive(Clone, Data, Debug)]
enum StatusType {
    Label(String),
    Progress(String, f64),
}

fn status_type(status: &AsyncOpsStatus) -> StatusType {
    fn f_name(p: &Path) -> Cow<str> {
        p.file_name()
            .map(|s| s.to_string_lossy())
            .unwrap_or("".into())
    }
    // We prioritize "in progress" messages.
    if let Some(x) = status.in_progress.encoding {
        StatusType::Progress("Encoding: ".to_owned(), x)
    } else if let Some(path) = &status.in_progress.saving {
        StatusType::Label(format!("Saving {}...", f_name(path)))
    } else if let Some(path) = &status.in_progress.loading {
        StatusType::Label(format!("Loading {}...", f_name(path)))
    } else if let Some(finished) = &status.last_finished {
        match finished {
            FinishedStatus::Saved { path, time: _ } => {
                StatusType::Label(format!("Saved {}", f_name(path)))
            }
            FinishedStatus::Loaded { path, time: _ } => {
                StatusType::Label(format!("Loaded {}", f_name(path)))
            }
            FinishedStatus::Encoded { path, time: _ } => {
                StatusType::Label(format!("Encoded {}", f_name(path)))
            }
            FinishedStatus::Error(s) => StatusType::Label(format!("Error: {}", s)),
        }
    } else {
        StatusType::Label(String::new())
    }
}

pub fn make_status_bar() -> impl Widget<EditorState> {
    let time_label = Label::new(|data: &EditorState, _env: &Env| {
        let usecs = data.time().as_micros();
        let mins = usecs / 60_000_000;
        let secs = (usecs / 1_000_000) % 60;
        let cents = (usecs / 10_000) % 100;
        format!("{:02}:{:02}.{:02}", mins, secs, cents)
    });

    let label = Label::dynamic(
        |data: &AsyncOpsStatus, _env: &Env| match status_type(data) {
            StatusType::Label(s) => s.to_owned(),
            StatusType::Progress(_, _) => String::new(),
        },
    );

    let label_progress =
        Label::dynamic(
            |data: &AsyncOpsStatus, _env: &Env| match status_type(data) {
                StatusType::Progress(s, _) => s.to_owned(),
                StatusType::Label(_) => String::new(),
            },
        );

    let progress = ProgressBar::new().lens(lens::Id.map(
        |s| {
            if let StatusType::Progress(_, x) = status_type(s) {
                x
            } else {
                0.0
            }
        },
        |_, _| {},
    ));

    let status_label = Either::new(
        |data: &AsyncOpsStatus, _env| matches!(status_type(data), StatusType::Progress(_, _)),
        Flex::row()
            .with_child(label_progress)
            .with_child(progress)
            .with_flex_spacer(1.0),
        label,
    )
    .fix_width(250.0); // TODO: can we make this depend on the text width?

    let row = Flex::row()
        .with_child(time_label)
        .with_flex_spacer(1.0)
        .with_child(status_label.lens(EditorState::status));
    Align::centered(row)
}
