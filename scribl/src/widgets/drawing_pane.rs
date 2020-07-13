use druid::kurbo::TranslateScale;
use druid::{
    BoxConstraints, Color, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Vec2, Widget,
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

pub struct DrawingPane {
    paper_rect: Rect,
    cursor: SnippetsCursor,
    /// Which point of the image should be visible at the top-left of the region?
    /// (This is used to derive `paper_rect`, which is then the authoritative source for answering
    /// this question, because it might contain some adjustments due to aspect ratio).
    offset: Vec2,
    /// The last interesting position of the mouse (used for figuring out how much to pan by).
    last_mouse_pos: Point,
}

impl DrawingPane {
    fn to_image_coords(&self) -> TranslateScale {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        let size_ratio = DRAWING_WIDTH / self.paper_rect.width();
        TranslateScale::scale(size_ratio) * TranslateScale::translate(-top_left)
    }

    fn from_image_coords(&self) -> TranslateScale {
        let top_left = Vec2::new(self.paper_rect.x0, self.paper_rect.y0);
        TranslateScale::translate(top_left) * TranslateScale::scale(self.from_image_scale())
    }

    fn from_image_scale(&self) -> f64 {
        self.paper_rect.width() / DRAWING_WIDTH
    }

    fn recompute_paper_rect(&mut self, size: Size, zoom: f64) {
        // Find the largest rectangle of the correct aspect ratio that will fit in the size.
        let paper_width = size.width.min(ASPECT_RATIO * size.height);
        let paper_height = paper_width / ASPECT_RATIO;
        let mut rect = Size::new(paper_width, paper_height).to_rect();

        rect = TranslateScale::scale(zoom) * rect;

        // The basic translate puts `self.offset` at the top-left of the view, however...
        let mut translate = -self.offset * zoom;
        // ...we don't want to leave blank space near the top-left...
        translate.x = translate.x.min(0.0);
        translate.y = translate.y.min(0.0);
        // ...or near the bottom-right...
        translate.x = translate.x.max(size.width - rect.width());
        translate.y = translate.y.max(size.height - rect.height());
        // ...and if there is spare room in either dimension, center it in that dimension.
        if rect.width() < size.width {
            translate.x = (size.width - rect.width()) / 2.0;
        }
        if rect.height() < size.height {
            translate.y = (size.height - rect.height()) / 2.0;
        }

        self.offset = -translate / zoom;
        rect = TranslateScale::translate(translate) * rect;

        // Rounding helps us align better with the pixels.
        self.paper_rect = rect.round();
    }
}

impl Default for DrawingPane {
    fn default() -> DrawingPane {
        DrawingPane {
            paper_rect: Rect::ZERO,
            cursor: SnippetsCursor::empty(Time::ZERO),
            offset: Vec2::ZERO,
            last_mouse_pos: Point::ZERO,
        }
    }
}

impl Widget<EditorState> for DrawingPane {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut EditorState, _env: &Env) {
        match event {
            Event::MouseMove(ev) => {
                if ctx.is_active() {
                    if data.action.is_recording() {
                        let time = data.accurate_time();

                        // Compute the rectangle that needs to be invalidated in order to draw this new
                        // point.
                        let mut invalid = Rect::from_origin_size(ev.pos, (0.0, 0.0));
                        let last_point = data.new_stroke.as_ref().and_then(|s| s.last_point());
                        if let Some(last_point) = last_point {
                            invalid = invalid.union_pt(self.from_image_coords() * last_point);
                        }
                        let pen_width = data.pen_size.size_fraction() * self.from_image_scale();
                        ctx.request_paint_rect(invalid.inset(pen_width).expand());

                        data.add_to_cur_snippet(self.to_image_coords() * ev.pos, time);
                    } else {
                        // Pan the view.
                        self.offset -= (ev.pos - self.last_mouse_pos) / data.zoom;
                        self.recompute_paper_rect(ctx.size(), data.zoom);
                        ctx.request_paint();
                        // TODO: change the mouse cursor
                    }
                    self.last_mouse_pos = ev.pos;
                }
            }
            Event::MouseDown(ev) if ev.button.is_left() => {
                ctx.set_active(true);
                self.last_mouse_pos = ev.pos;
                if let CurrentAction::WaitingToRecord(_) = data.action {
                    data.start_actually_recording();
                    ctx.request_anim_frame();
                }
                if data.action.is_recording() {
                    let time = data.accurate_time();
                    data.add_to_cur_snippet(self.to_image_coords() * ev.pos, time);
                }
            }
            Event::MouseUp(ev) => {
                ctx.set_active(false);
                if ev.button.is_left() && data.action.is_recording() {
                    if let Some(seg) = data.finish_cur_segment() {
                        ctx.submit_command(Command::new(cmd::APPEND_NEW_SEGMENT, seg), None);
                    }
                }
            }
            Event::Wheel(ev) => {
                let zoom = (data.zoom * (-ev.wheel_delta.y / 500.0).exp())
                    .max(1.0)
                    .min(crate::editor_state::MAX_ZOOM);
                let zoom_factor = zoom / data.zoom;

                // Try to translate so that the mouse stays over whatever part of the drawing it's
                // currently over.
                self.offset += ev.pos.to_vec2() / data.zoom * (zoom_factor - 1.0);
                data.zoom = zoom;
                self.recompute_paper_rect(ctx.size(), zoom);
                ctx.request_paint();
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
        if !old_data.snippets.same(&data.snippets) {
            self.cursor = data.snippets.create_cursor(data.time());
            ctx.request_paint();
        } else if old_data.time() != data.time() {
            self.cursor.advance_to(
                old_data.time().min(data.time()),
                old_data.time().max(data.time()),
            );
            // It doesn't matter whether we use the new snippets or the old snippets, because if
            // they differ then we didn't get here.
            // TODO: consider invalidating everything if there are many bboxes.
            let transform = self.from_image_coords();
            for bbox in self.cursor.bboxes(&data.snippets) {
                ctx.request_paint_rect(transform * bbox);
            }

            self.cursor.advance_to(data.time(), data.time());
        }

        if old_data.zoom != data.zoom {
            self.recompute_paper_rect(ctx.size(), data.zoom);
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &EditorState,
        _env: &Env,
    ) {
        if matches!(event, LifeCycle::WidgetAdded) {
            self.cursor = data.snippets.create_cursor(data.time());
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        _env: &Env,
    ) -> Size {
        let size = bc.max();
        self.recompute_paper_rect(size, data.zoom);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, _env: &Env) {
        let size = ctx.size();
        ctx.with_save(|ctx| {
            ctx.clip(size.to_rect());
            ctx.fill(&self.paper_rect, &PAPER_COLOR);

            ctx.transform(self.from_image_coords().into());
            for id in self.cursor.active_ids() {
                data.snippets
                    .snippet(id)
                    .render(ctx.render_ctx, data.time());
            }
            if let Some(curve) = data.new_stroke_seq.as_ref() {
                curve.render(ctx.render_ctx, data.time());
            }
            if let Some(snip) = data.new_stroke.as_ref() {
                // FIXME: there's an invalidation bug with fade on the new stroke.
                snip.render(ctx.render_ctx, data.cur_style(), data.time());
            }
        });
    }
}
