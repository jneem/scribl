use druid::{
    BoxConstraints, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx, PaintCtx,
    RenderContext, Size, TimerToken, UpdateCtx, Widget,
};
use std::convert::TryInto;
use std::time::Instant;

use crate::data::{CurrentAction, CurveInProgress, ScribbleState};
use crate::FRAME_TIME;

pub struct DrawingPane {
    timer_id: TimerToken,
}

impl Default for DrawingPane {
    fn default() -> DrawingPane {
        DrawingPane {
            timer_id: TimerToken::INVALID,
        }
    }
}

impl Widget<ScribbleState> for DrawingPane {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, state: &mut ScribbleState, _env: &Env) {
        match event {
            Event::MouseMoved(ev) => {
                if state.mouse_down && state.action.is_recording() {
                    state.new_snippet.as_mut().unwrap().line_to(ev.pos);
                    ctx.request_paint();
                }
            }
            Event::MouseDown(ev) => {
                if ev.button.is_left() && state.action.is_recording() {
                    if state.new_snippet.is_none() {
                        state.new_snippet = Some(CurveInProgress::new(state.time_us));
                    }

                    let snip = state.new_snippet.as_mut().unwrap();
                    snip.move_to(ev.pos);

                    state.mouse_down = true;
                    ctx.request_paint();
                }
            }
            Event::MouseUp(ev) => {
                if ev.button.is_left() && state.action.is_recording() {
                    state.mouse_down = false;
                }
            }
            Event::WindowConnected => {
                ctx.request_paint();
                self.timer_id = ctx.request_timer(Instant::now() + FRAME_TIME);
            }
            Event::Timer(tok) => {
                if tok == &self.timer_id && !state.action.is_idle() {
                    let frame_time_micros: i64 = FRAME_TIME.as_micros().try_into().unwrap();
                    state.time_us += frame_time_micros;
                    ctx.request_paint();
                }

                self.timer_id = ctx.request_timer(Instant::now() + FRAME_TIME);
            }
            _ => {}
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_state: &ScribbleState,
        _state: &ScribbleState,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _: &LifeCycle,
        _state: &ScribbleState,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &ScribbleState,
        _env: &Env,
    ) -> Size {
        dbg!(bc);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, _env: &Env) {
        if let Some(curve) = data.curve_in_progress() {
            ctx.stroke(&curve.path, &curve.color, curve.thickness);
        }

        for curve in &data.snippets.borrow().curves {
            ctx.stroke(
                curve.path_until(data.time_us),
                &curve.curve.color,
                curve.curve.thickness,
            );
        }
    }
}
