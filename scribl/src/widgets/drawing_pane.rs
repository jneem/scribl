use druid::{
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Vec2, Widget,
};

use scribl_curves::{SnippetsCursor, Time};

use crate::cmd;
use crate::editor_state::{CurrentAction, EditorState};

// The drawing coordinates are chosen so that the width of the image is always
// 1.0. For now we also fix the height, but eventually we will support other aspect
// ratios.
pub const DRAWING_WIDTH: f64 = 1.0;
pub const DRAWING_HEIGHT: f64 = 0.75;

const ASPECT_RATIO: f64 = DRAWING_WIDTH / DRAWING_HEIGHT;
const PAPER_COLOR: Color = Color::rgb8(0xff, 0xff, 0xff);
const PAPER_BDY_COLOR: Color = Color::rgb8(0x00, 0x00, 0x00);
const PAPER_BDY_THICKNESS: f64 = 1.0;

pub struct DrawingPane {
    paper_rect: Rect,
    cursor: SnippetsCursor,
}

impl DrawingPane {
    fn to_image_coords(&self) -> Affine {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        let size_ratio = DRAWING_WIDTH / self.paper_rect.width();
        Affine::scale(size_ratio) * Affine::translate(-top_left)
    }

    fn from_image_coords(&self) -> Affine {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        Affine::translate(top_left) * Affine::scale(self.from_image_scale())
    }

    fn from_image_scale(&self) -> f64 {
        self.paper_rect.width() / DRAWING_WIDTH
    }
}

impl Default for DrawingPane {
    fn default() -> DrawingPane {
        DrawingPane {
            paper_rect: Rect::ZERO,
            cursor: SnippetsCursor::empty(Time::ZERO),
        }
    }
}

impl Widget<EditorState> for DrawingPane {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, state: &mut EditorState, _env: &Env) {
        match event {
            Event::MouseMove(ev) => {
                if ctx.is_active() && state.action.is_recording() {
                    let time = state.accurate_time();

                    // Compute the rectangle that needs to be invalidated in order to draw this new
                    // point.
                    let mut invalid = Rect::from_origin_size(ev.pos, (0.0, 0.0));
                    let last_point = state.new_stroke.as_ref().and_then(|s| s.last_point());
                    if let Some(last_point) = last_point {
                        invalid = invalid.union_pt(last_point);
                    }
                    let pen_width = state.pen_size.size_fraction() * self.from_image_scale();
                    ctx.request_paint_rect(invalid.inset(pen_width).expand());

                    state.add_to_cur_snippet(self.to_image_coords() * ev.pos, time);
                }
            }
            Event::MouseDown(ev) if ev.button.is_left() => {
                if let CurrentAction::WaitingToRecord(_) = state.action {
                    state.start_actually_recording();
                }
                if state.action.is_recording() {
                    let time = state.accurate_time();
                    state.add_to_cur_snippet(self.to_image_coords() * ev.pos, time);

                    ctx.set_active(true);
                }
            }
            Event::MouseUp(ev) => {
                if ev.button.is_left() && state.action.is_recording() {
                    ctx.set_active(false);
                    if let Some(seg) = state.finish_cur_segment() {
                        ctx.submit_command(Command::new(cmd::APPEND_NEW_SEGMENT, seg), None);
                    }
                }
            }
            Event::WindowConnected => {
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        _env: &Env,
    ) {
        if old_data.time() != data.time() {
            self.cursor.advance_to(data.time());
            // TODO: figure out the region that changed
            ctx.request_paint();
        }

        if !old_data.snippets.same(&data.snippets) {
            self.cursor = data.snippets.create_cursor(data.time());
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _: &LifeCycle,
        _state: &EditorState,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &EditorState,
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, _env: &Env) {
        ctx.stroke(&self.paper_rect, &PAPER_BDY_COLOR, PAPER_BDY_THICKNESS);
        ctx.fill(&self.paper_rect, &PAPER_COLOR);

        ctx.with_save(|ctx| {
            ctx.transform(self.from_image_coords());
            for (_, snip) in data.snippets.snippets() {
                snip.render(ctx.render_ctx, data.time());
            }
            if let Some(curve) = data.new_stroke_seq.as_ref() {
                curve.render(ctx.render_ctx, data.time());
            }
            if let Some(snip) = data.new_stroke.as_ref() {
                snip.render(ctx.render_ctx, data.cur_style(), data.time());
            }
        });
    }
}
