use druid::widget::{Align, Flex};
use druid::{
    BoxConstraints, Env, Event, EventCtx, KeyCode, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    Size, TimerToken, UpdateCtx, Widget,
};
use std::convert::TryInto;
use std::time::Instant;

use crate::data::{CurrentAction, ScribbleState};
use crate::widgets::{DrawingPane, Timeline, ToggleButton};
use crate::FRAME_TIME;

fn rec_button_on(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_recording();
}

fn rec_button_off(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    dbg!("Stopped recording", data.time_us);
    data.stop_recording();
}

fn play_button_on(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.start_playing();
}

fn play_button_off(_ctx: &mut EventCtx, data: &mut ScribbleState, _env: &Env) {
    data.stop_playing();
}

pub struct Root {
    timer_id: TimerToken,
    inner: Box<dyn Widget<ScribbleState>>,
}

impl Root {
    pub fn new() -> Root {
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
            .with_spacer(10.0)
            .with_child(drawing, 1.0)
            .with_spacer(10.0)
            .with_child(Timeline::default(), 0.0);

        Root {
            inner: Box::new(Align::centered(column)),
            timer_id: TimerToken::INVALID,
        }
    }
}

impl Widget<ScribbleState> for Root {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut ScribbleState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_focus();
                ctx.request_paint();
                self.timer_id = ctx.request_timer(Instant::now() + FRAME_TIME);
            }
            Event::KeyDown(ev) => {
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
                        data.mark = Some(data.time_us);
                    }
                    KeyCode::KeyT => {
                        if let Some(snip) = data.selected_snippet {
                            data.snippets =
                                data.snippets.with_truncated_snippet(snip, data.time_us);
                        }
                    }
                    KeyCode::KeyW => {
                        if let Some(mark_time) = data.mark {
                            if let Some(snip) = data.selected_snippet {
                                data.snippets =
                                    data.snippets.with_new_lerp(snip, data.time_us, mark_time);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Event::KeyUp(ev) => match ev.key_code {
                KeyCode::ArrowRight | KeyCode::ArrowLeft => {
                    if data.action.is_scanning() {
                        data.action = CurrentAction::Idle;
                    }
                    ctx.set_handled();
                }
                _ => {}
            },
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

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &ScribbleState,
        data: &ScribbleState,
        env: &Env,
    ) {
        self.inner.update(ctx, old_data, data, env);
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &ScribbleState,
        env: &Env,
    ) {
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ScribbleState,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, env: &Env) {
        self.inner.paint(ctx, data, env);
    }
}
