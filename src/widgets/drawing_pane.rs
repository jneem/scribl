use druid::{BoxConstraints, Env, Event, EventCtx, LifeCycle, LifeCycleCtx, Size, UpdateCtx, PaintCtx, Widget, LayoutCtx, RenderContext};

use crate::data::{CurveInProgress, ScribbleState};


#[derive(Default)]
pub struct DrawingPane;

impl Widget<ScribbleState> for DrawingPane {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, state: &mut ScribbleState, _env: &Env) {
        match event {
            Event::MouseMoved(ev) => {
                if state.mouse_down && state.action.is_recording() {
                    dbg!(ev);
                    state.new_snippet.as_mut().unwrap().line_to(ev.pos);
                    ctx.request_paint();
                }
            },
            Event::MouseDown(ev) => {
                if ev.button.is_left() && state.action.is_recording() {
                    if state.new_snippet.is_none() {
                        state.new_snippet = Some(CurveInProgress::new(state.time_us));
                    }

                    let snip = state.new_snippet.as_mut().unwrap();
                    snip.initialize_if_necessary(state.time_us);
                    snip.move_to(ev.pos);

                    state.mouse_down = true;
                    ctx.request_paint();
                }
            },
            Event::MouseUp(ev) => {
                if ev.button.is_left() && state.action.is_recording() {
                    state.mouse_down = false;
                }
            }
            _ => {},
        }
    }

    fn update(&mut self, _ctx: &mut UpdateCtx, _old_state: &ScribbleState, _state: &ScribbleState, _env: &Env) {
    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _: &LifeCycle, _state: &ScribbleState, _env: &Env) {
    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, _data: &ScribbleState, _env: &Env) -> Size {
        dbg!(bc);
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, _env: &Env) {
        if let Some(curve) = data.curve_in_progress() {
            ctx.stroke(&curve.path, &curve.color, curve.thickness);
        }
    }
}

