use druid::widget::prelude::*;
use druid::widget::{Button, Controller, Flex, Label, Spinner};
use druid::{SingleUse, Widget, WidgetExt};

use scribl_widget::ModalHost;

use crate::{CurrentAction, EditorState};

pub fn make_unsaved_changes_alert() -> impl Widget<EditorState> {
    let close =
        Button::new("Close without saving").on_click(|ctx, data: &mut EditorState, _env| {
            data.action = CurrentAction::WaitingToExit;
            ctx.submit_command(ModalHost::DISMISS_MODAL);
            ctx.submit_command(druid::commands::CLOSE_WINDOW);
        });

    let cancel = Button::new("Cancel").on_click(|ctx, _data, _env| {
        ctx.submit_command(ModalHost::DISMISS_MODAL);
    });
    let save = Button::dynamic(|data: &EditorState, _| {
        if data.save_path.is_some() {
            "Save".to_owned()
        } else {
            "Save as".to_owned()
        }
    })
    .on_click(|ctx, data, _env| {
        ctx.submit_command(ModalHost::DISMISS_MODAL);
        if data.save_path.is_some() {
            ctx.submit_command(druid::commands::SAVE_FILE);
        } else {
            ctx.submit_command(
                druid::commands::SHOW_SAVE_PANEL.with(crate::menus::save_dialog_options()),
            );
        }
        data.action = CurrentAction::WaitingToExit;
        ctx.submit_command(
            ModalHost::SHOW_MODAL.with(SingleUse::new(Box::new(make_waiting_to_exit_alert()))),
        );
    });

    let button_row = Flex::row()
        .with_child(close)
        .with_spacer(5.0)
        .with_child(cancel)
        .with_spacer(5.0)
        .with_child(save);

    let label = Label::dynamic(|data: &EditorState, _| {
        if let Some(file_name) = data
            .save_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|f| f.to_string_lossy())
        {
            format!("\"{}\" has unsaved changes!", file_name)
        } else {
            "Your untitled animation has unsaved changes!".to_owned()
        }
    });

    Flex::column()
        .with_child(label)
        .with_spacer(15.0)
        .with_child(button_row)
        .padding(10.0)
        .background(druid::theme::BACKGROUND_LIGHT)
        .border(druid::theme::FOREGROUND_DARK, 1.0)
}

/// This controller gets instantiated when we're planning to close a window. Its job is to sit and
/// wait until any saves and encodes in progress are finished. When they are, it sends a
/// CLOSE_WINDOW command.
struct Waiter {}

impl<W: Widget<EditorState>> Controller<EditorState, W> for Waiter {
    fn update(
        &mut self,
        child: &mut W,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if data.status.in_progress.saving.is_none() && data.status.in_progress.encoding.is_none() {
            ctx.submit_command(druid::commands::CLOSE_WINDOW);
        }
        child.update(ctx, old_data, data, env);
    }

    fn lifecycle(
        &mut self,
        child: &mut W,
        ctx: &mut LifeCycleCtx,
        ev: &LifeCycle,
        data: &EditorState,
        env: &Env,
    ) {
        // We check for termination in lifecycle as well as update, because it's possible that
        // the condition was triggered before we were instantiated, in which case we'll get a
        // lifecycle event when we're added to the widget tree but we won't get any updates.
        if data.status.in_progress.saving.is_none() && data.status.in_progress.encoding.is_none() {
            ctx.submit_command(druid::commands::CLOSE_WINDOW);
        }
        child.lifecycle(ctx, ev, data, env);
    }
}

pub fn make_waiting_to_exit_alert() -> impl Widget<EditorState> {
    let label = Label::dynamic(|data: &EditorState, _env| {
        if let Some(progress) = data.status.in_progress.encoding {
            format!("Encoding (frame {} of {})...", progress.0, progress.1)
        } else {
            "Saving...".to_owned()
        }
    });

    let spinner = Spinner::new();

    Flex::column()
        .with_child(label)
        .with_spacer(15.0)
        .with_child(spinner)
        .padding(10.0)
        .background(druid::theme::BACKGROUND_LIGHT)
        .border(druid::theme::FOREGROUND_DARK, 1.0)
        .controller(Waiter {})
}
