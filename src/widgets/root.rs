use druid::widget::{Align, Flex};
use druid::{
    BoxConstraints, Color, Command, Env, Event, EventCtx, KeyCode, KeyEvent, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Size, TimerToken, UpdateCtx, Widget, WidgetExt,
};
use std::convert::TryInto;
use std::time::Instant;

use crate::audio::AudioSnippetData;
use crate::cmd;
use crate::data::{AppState, CurrentAction, ScribbleState, SnippetData};
use crate::undo::UndoStack;
use crate::widgets::{DrawingPane, Palette, Timeline, ToggleButton};
use crate::FRAME_TIME;

pub struct Root {
    timer_id: TimerToken,
    inner: Box<dyn Widget<AppState>>,
    undo: UndoStack,
}

impl Root {
    pub fn new(scribble_state: ScribbleState) -> Root {
        let drawing = DrawingPane::default();
        let rec_button: ToggleButton<AppState> = ToggleButton::new(
            "Rec",
            |state: &AppState| state.action.rec_toggle(),
            |_, data, _| data.start_recording(),
            |ctx, data, _| {
                if let Some(new_snippet) = data.stop_recording() {
                    ctx.submit_command(Command::new(cmd::ADD_SNIPPET, new_snippet), None);
                }
            },
        );
        let rec_audio_button: ToggleButton<AppState> = ToggleButton::new(
            "Audio",
            |state: &AppState| state.action.rec_audio_toggle(),
            |_, data, _| data.start_recording_audio(),
            |ctx, data, _| {
                let snip = data.stop_recording_audio();
                ctx.submit_command(Command::new(cmd::ADD_AUDIO_SNIPPET, snip), None);
            },
        );
        let play_button = ToggleButton::new(
            "Play",
            |state: &AppState| state.action.play_toggle(),
            |_, data, _| data.start_playing(),
            |_, data, _| data.stop_playing(),
        );

        let palette = Palette::default();

        let button_row = Flex::row()
            .with_child(rec_button)
            .with_child(rec_audio_button)
            .with_child(play_button)
            .with_flex_spacer(1.0)
            .with_child(palette.lens(AppState::palette));
        let column = Flex::column()
            .with_child(button_row)
            .with_spacer(10.0)
            .with_flex_child(drawing, 1.0)
            .with_spacer(10.0)
            .with_child(Timeline::default());

        Root {
            inner: Box::new(Align::centered(column)),
            timer_id: TimerToken::INVALID,
            undo: UndoStack::new(scribble_state),
        }
    }
}

impl Root {
    fn handle_key_down(
        &mut self,
        ctx: &mut EventCtx,
        ev: &KeyEvent,
        data: &mut AppState,
        _env: &Env,
    ) {
        // If they push another key while holding down the arrow, cancel the scanning.
        if let CurrentAction::Scanning(speed) = data.action {
            let direction = if speed > 0.0 {
                KeyCode::ArrowRight
            } else {
                KeyCode::ArrowLeft
            };
            if ev.key_code != direction {
                data.action = CurrentAction::Idle;
            }
            ctx.set_handled();
            if ev.key_code == KeyCode::ArrowRight || ev.key_code == KeyCode::ArrowLeft {
                return;
            }
        }

        // TODO: do some of these keys as commands
        match ev.key_code {
            KeyCode::ArrowRight | KeyCode::ArrowLeft => {
                if data.action.is_idle() {
                    let speed = if ev.mods.shift { 2.0 } else { 1.0 };
                    let dir = if ev.key_code == KeyCode::ArrowRight {
                        1.0
                    } else {
                        -1.0
                    };
                    data.action = CurrentAction::Scanning(speed * dir);
                }
                ctx.set_handled();
            }
            KeyCode::KeyM => {
                ctx.submit_command(Command::new(cmd::SET_MARK, data.time_us), None);
                ctx.set_handled();
            }
            KeyCode::KeyT => {
                if let Some(snip) = data.scribble.selected_snippet {
                    ctx.submit_command(
                        Command::new(
                            cmd::TRUNCATE_SNIPPET,
                            cmd::TruncateSnippetCmd {
                                id: snip,
                                time_us: data.time_us,
                            },
                        ),
                        None,
                    );
                    ctx.set_handled();
                }
            }
            KeyCode::KeyW => {
                if let Some(mark_time) = data.scribble.mark {
                    if let Some(snip) = data.scribble.selected_snippet {
                        ctx.submit_command(
                            Command::new(
                                cmd::LERP_SNIPPET,
                                cmd::LerpSnippetCmd {
                                    id: snip,
                                    from_time: data.time_us,
                                    to_time: mark_time,
                                },
                            ),
                            None,
                        );
                    }
                }
            }
            _ => {}
        }
    }

    fn handle_key_up(
        &mut self,
        ctx: &mut EventCtx,
        ev: &KeyEvent,
        data: &mut AppState,
        _env: &Env,
    ) {
        match ev.key_code {
            KeyCode::ArrowRight | KeyCode::ArrowLeft => {
                if data.action.is_scanning() {
                    data.action = CurrentAction::Idle;
                }
                ctx.set_handled();
            }
            _ => {}
        }
    }

    fn handle_command(
        &mut self,
        ctx: &mut EventCtx,
        cmd: &Command,
        data: &mut AppState,
        _env: &Env,
    ) -> bool {
        match cmd.selector {
            cmd::ADD_SNIPPET => {
                let snip = cmd.get_object::<SnippetData>().expect("no snippet");
                let (new_snippets, new_id) = data.scribble.snippets.with_new_snippet(snip.clone());
                data.scribble.snippets = new_snippets;
                data.scribble.selected_snippet = Some(new_id);
                self.undo.push(&data.scribble);
                true
            }
            cmd::ADD_AUDIO_SNIPPET => {
                let snip = cmd
                    .get_object::<AudioSnippetData>()
                    .expect("no audio snippet");
                data.scribble.audio_snippets =
                    data.scribble.audio_snippets.with_new_snippet(snip.clone());
                self.undo.push(&data.scribble);
                true
            }
            cmd::CHOOSE_COLOR => {
                let color = cmd.get_object::<Color>().expect("API violation");
                data.palette.select(color);
                true
            }
            cmd::SET_MARK => {
                let time = cmd.get_object::<i64>().expect("API violation");
                data.scribble.mark = Some(*time);
                self.undo.push(&data.scribble);
                true
            }
            cmd::TRUNCATE_SNIPPET => {
                let cmd = cmd
                    .get_object::<cmd::TruncateSnippetCmd>()
                    .expect("API violation");
                data.scribble.snippets = data
                    .scribble
                    .snippets
                    .with_truncated_snippet(cmd.id, cmd.time_us);
                self.undo.push(&data.scribble);
                true
            }
            cmd::LERP_SNIPPET => {
                let cmd = cmd
                    .get_object::<cmd::LerpSnippetCmd>()
                    .expect("API violation");
                data.scribble.snippets =
                    data.scribble
                        .snippets
                        .with_new_lerp(cmd.id, cmd.from_time, cmd.to_time);
                self.undo.push(&data.scribble);
                true
            }
            druid::commands::UNDO => {
                if let Some(undone_state) = self.undo.undo() {
                    data.scribble = undone_state;
                    ctx.request_paint();
                }
                true
            }
            druid::commands::REDO => {
                if let Some(redone_state) = self.undo.redo() {
                    data.scribble = redone_state;
                    ctx.request_paint();
                }
                true
            }
            _ => false,
        }
    }
}

impl Widget<AppState> for Root {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
                ctx.request_paint();
                self.timer_id = ctx.request_timer(Instant::now() + FRAME_TIME);
            }
            Event::Command(cmd) => {
                let handled = self.handle_command(ctx, cmd, data, env);
                if handled {
                    ctx.set_handled();
                }
            }
            Event::KeyDown(ev) => self.handle_key_down(ctx, ev, data, env),
            Event::KeyUp(ev) => self.handle_key_up(ctx, ev, data, env),
            Event::Timer(tok) => {
                if tok == &self.timer_id {
                    let frame_time_us: i64 = if data.action.is_ticking() {
                        FRAME_TIME.as_micros().try_into().unwrap()
                    } else if let CurrentAction::Scanning(speed) = data.action {
                        let t = FRAME_TIME.as_micros() as f64;
                        (t * speed) as i64
                    } else {
                        0
                    };
                    data.time_us = (data.time_us + frame_time_us).max(0);
                }

                self.timer_id = ctx.request_timer(Instant::now() + FRAME_TIME);
            }
            _ => {
                self.inner.event(ctx, event, data, env);
            }
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, env: &Env) {
        self.inner.update(ctx, old_data, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &AppState, env: &Env) {
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &AppState,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, env: &Env) {
        self.inner.paint(ctx, data, env);
    }
}
