use druid::{
    Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Vec2, Widget,
};

use scribble_curves::SnippetsCursor;

use crate::data::{AppState, CurrentAction};

// Width/height of the drawing in image coordinates.
pub const DRAWING_WIDTH: u64 = 1600;
pub const DRAWING_HEIGHT: u64 = 1200;

const ASPECT_RATIO: f64 = (DRAWING_WIDTH as f64) / (DRAWING_HEIGHT as f64);
const PAPER_COLOR: Color = Color::rgb8(0xff, 0xff, 0xff);
const PAPER_BDY_COLOR: Color = Color::rgb8(0x00, 0x00, 0x00);
const PAPER_BDY_THICKNESS: f64 = 1.0;

pub struct DrawingPane {
    paper_rect: Rect,
    cursor: Option<SnippetsCursor>,
}

impl DrawingPane {
    fn to_image_coords(&self) -> Affine {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        let size_ratio = (DRAWING_WIDTH as f64) / self.paper_rect.width();
        Affine::scale(size_ratio) * Affine::translate(-top_left)
    }

    fn from_image_coords(&self) -> Affine {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        let size_ratio = (DRAWING_WIDTH as f64) / self.paper_rect.width();
        Affine::translate(top_left) * Affine::scale(1.0 / size_ratio)
    }
}

impl Default for DrawingPane {
    fn default() -> DrawingPane {
        DrawingPane {
            paper_rect: Rect::ZERO,
            cursor: None,
        }
    }
}

// TODO: can we do ScribbleState instead of AppState?
impl Widget<AppState> for DrawingPane {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, state: &mut AppState, _env: &Env) {
        match event {
            Event::MouseMoved(ev) => {
                if state.mouse_down && state.action.is_recording() {
                    // TODO: get the time with higher resolution by measuring the time elapsed
                    // since the last tick
                    state
                        .scribble
                        .new_snippet
                        .as_mut()
                        .unwrap()
                        .line_to(self.to_image_coords() * ev.pos, state.time);
                    ctx.request_paint();
                }
            }
            Event::MouseDown(ev) if ev.button.is_left() => {
                if let CurrentAction::WaitingToRecord(time_factor) = state.action {
                    state.action = CurrentAction::Recording(time_factor);
                }
                if state.action.is_recording() {
                    let snip = state
                        .scribble
                        .new_snippet
                        .as_mut()
                        .expect("Recording, but no snippet!");
                    // TODO: get the time with higher resolution by measuring the time elapsed
                    // since the last tick
                    snip.move_to(self.to_image_coords() * ev.pos, state.time);

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
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, _env: &Env) {
        if old_data.time != data.time {
            ctx.request_paint();
        }

        if !old_data.scribble.snippets.same(&data.scribble.snippets) {
            self.cursor = Some(data.scribble.snippets.create_cursor(data.time));
            ctx.request_paint();
        }
    }

    fn lifecycle(&mut self, _ctx: &mut LifeCycleCtx, _: &LifeCycle, _state: &AppState, _env: &Env) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &AppState,
        _env: &Env,
    ) -> Size {
        let size = bc.max();

        // Find the largest rectangle of the correct aspect ratio that will fit in the box.
        let paper_width = size.width.min(ASPECT_RATIO * size.height);
        let paper_height = paper_width / ASPECT_RATIO;
        self.paper_rect = Rect::from_origin_size(Point::ZERO, (paper_width, paper_height));
        self.paper_rect =
            self.paper_rect + size.to_vec2() / 2.0 - self.paper_rect.center().to_vec2();
        self.paper_rect = self.paper_rect.inset(PAPER_BDY_THICKNESS).round();

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, _env: &Env) {
        ctx.stroke(&self.paper_rect, &PAPER_BDY_COLOR, PAPER_BDY_THICKNESS);
        ctx.fill(&self.paper_rect, &PAPER_COLOR);

        ctx.with_save(|ctx| {
            ctx.transform(self.from_image_coords());
            if let Some(curve) = data.scribble.curve_in_progress() {
                curve.render(ctx.render_ctx, data.time);
            }

            for (_, snip) in data.scribble.snippets.snippets() {
                snip.render(ctx.render_ctx, data.time);
            }
        });
    }
}
