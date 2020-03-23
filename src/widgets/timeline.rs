use druid::kurbo::{BezPath, Line};
use druid::theme;
use druid::{
    BoxConstraints, Color, Env, Event, EventCtx, LayoutCtx, Lens, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetPod,
};
use std::collections::HashMap;

use crate::snippet::{LerpedCurve, SnippetId, Snippets};
use crate::ScribbleState;

const SNIPPET_HEIGHT: f64 = 20.0;
const NUM_SNIPPETS: f64 = 5.0;
const MIN_WIDTH: f64 = 100.0;
const PIXELS_PER_USEC: f64 = 40.0 / 1000000.0;
const TIMELINE_BG_COLOR: Color = Color::rgb8(0x66, 0x66, 0x66);
const CURSOR_COLOR: Color = Color::rgb8(0x10, 0x10, 0xaa);
const CURSOR_THICKNESS: f64 = 3.0;

const SNIPPET_COLOR: Color = Color::rgb8(0x99, 0x99, 0x22);
const SNIPPET_SELECTED_COLOR: Color = Color::rgb8(0x77, 0x77, 0x11);
const SNIPPET_STROKE_COLOR: Color = Color::rgb8(0x22, 0x22, 0x22);
const SNIPPET_HOVER_STROKE_COLOR: Color = Color::rgb8(0, 0, 0);
const SNIPPET_STROKE_THICKNESS: f64 = 1.0;

const MARK_COLOR: Color = Color::rgb8(0x33, 0x33, 0x99);

fn timeline_snippet_same(c: &LerpedCurve, d: &LerpedCurve) -> bool {
    c.lerp == d.lerp
}

#[derive(Default)]
pub struct Timeline {
    snippet_offsets: HashMap<SnippetId, usize>,
    children: HashMap<SnippetId, WidgetPod<ScribbleState, TimelineSnippet>>,
}

impl Timeline {
    fn recalculate_snippet_offsets(&mut self, snippets: &Snippets) {
        self.snippet_offsets = snippets
            .layout_non_overlapping(NUM_SNIPPETS as usize)
            .expect("Couldn't fit all the snippets in!"); // FIXME: don't panic

        self.children.clear();
        for (id, _) in snippets.iter() {
            self.children
                .insert(id, WidgetPod::new(TimelineSnippet { id }));
        }
    }
}

struct TimelineSnippet {
    id: SnippetId,
}

struct SnippetLens(pub SnippetId);

impl Lens<ScribbleState, LerpedCurve> for SnippetLens {
    fn with<V, F: FnOnce(&LerpedCurve) -> V>(&self, data: &ScribbleState, f: F) -> V {
        f(&data.snippets.snippet(self.0))
    }

    fn with_mut<V, F: FnOnce(&mut LerpedCurve) -> V>(&self, data: &mut ScribbleState, f: F) -> V {
        f(&mut data.snippets.snippet_mut(self.0))
    }
}

impl TimelineSnippet {
    fn width(&self, data: &ScribbleState) -> f64 {
        let snippet = data.snippets.snippet(self.id);
        if let Some(end_time) = snippet.end_time() {
            (end_time - snippet.start_time()) as f64 * PIXELS_PER_USEC
        } else {
            std::f64::INFINITY
        }
    }
}

#[allow(unused_variables)]
impl Widget<ScribbleState> for TimelineSnippet {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut ScribbleState, _env: &Env) {
        match event {
            Event::MouseDown(ev) if ev.button.is_left() => {
                ctx.set_active(true);
                ctx.set_handled();
            }
            Event::MouseUp(ev) if ev.button.is_left() => {
                if ctx.is_active() && ctx.is_hot() {
                    data.selected_snippet = Some(self.id);
                    ctx.request_paint();
                    ctx.set_handled();
                }
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
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &ScribbleState,
        _env: &Env,
    ) {
        match event {
            LifeCycle::HotChanged(_) => {
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ScribbleState,
        _env: &Env,
    ) -> Size {
        let snippet = data.snippets.snippet(self.id);
        let width = self.width(data);
        let height = SNIPPET_HEIGHT;
        bc.constrain((width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, env: &Env) {
        let snippet = data.snippets.snippet(self.id);
        let width = self.width(data).min(10000.0); // FIXME: there are bugs drawing infinite rects.
        let height = SNIPPET_HEIGHT;
        let rect = Rect::from_origin_size(Point::ZERO, (width, height))
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let stroke_color = if ctx.is_hot() {
            &SNIPPET_STROKE_COLOR
        } else {
            &SNIPPET_HOVER_STROKE_COLOR
        };
        let fill_color = if data.selected_snippet == Some(self.id) {
            &SNIPPET_SELECTED_COLOR
        } else {
            &SNIPPET_COLOR
        };

        dbg!(rect);
        ctx.fill(&rect, fill_color);
        ctx.stroke(&rect, stroke_color, SNIPPET_STROKE_THICKNESS);

        // Draw the span of the edited region.
        let draw_width = (snippet.last_draw_time() - snippet.start_time()) as f64 * PIXELS_PER_USEC;
        let color = Color::rgb8(0, 0, 0);
        ctx.stroke(
            Line::new((0.0, height / 2.0), (draw_width, height / 2.0)),
            &color,
            1.0,
        );
        ctx.stroke(
            Line::new((draw_width, height * 0.25), (draw_width, height * 0.75)),
            &color,
            1.0,
        );
    }
}

impl Widget<ScribbleState> for Timeline {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut ScribbleState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_paint();
            }
            Event::MouseDown(ev) => {
                data.time_us = (ev.pos.x / PIXELS_PER_USEC) as i64;
                ctx.set_active(true);
                ctx.request_paint();
            }
            Event::MouseMoved(ev) => {
                // On click-and-drag, we change the time with the drag.
                if ctx.is_active() {
                    data.time_us = (ev.pos.x / PIXELS_PER_USEC) as i64;
                    ctx.request_paint();
                }
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                }
            }
            _ => {}
        }

        for child in self.children.values_mut() {
            child.event(ctx, event, data, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &ScribbleState,
        data: &ScribbleState,
        _env: &Env,
    ) {
        // TODO: do this better
        if data.snippets.snippets().curves().count() != self.children.len() {
            ctx.request_layout();
            self.recalculate_snippet_offsets(&data.snippets.snippets());
            ctx.children_changed();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &ScribbleState,
        env: &Env,
    ) {
        for child in self.children.values_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ScribbleState,
        env: &Env,
    ) -> Size {
        for (&id, &offset) in &self.snippet_offsets {
            let x = (data.snippets.snippet(id).lerp.first() as f64) * PIXELS_PER_USEC;
            let y = offset as f64 * SNIPPET_HEIGHT;

            // FIXME: shouldn't we modify bc before recursing?
            let child = self.children.get_mut(&id).unwrap();
            let size = child.layout(ctx, bc, data, env);
            child.set_layout_rect(dbg!(Rect::from_origin_size((x, y), size)));
        }

        let height = SNIPPET_HEIGHT * NUM_SNIPPETS;
        bc.constrain((std::f64::INFINITY, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &ScribbleState, env: &Env) {
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);
        ctx.fill(rect, &TIMELINE_BG_COLOR);

        for child in self.children.values_mut() {
            child.paint_with_offset(ctx, data, env);
        }

        // Draw the cursor.
        let cursor_x = PIXELS_PER_USEC * (data.time_us as f64);
        let line = Line::new((cursor_x, 0.0), (cursor_x, size.height));
        ctx.stroke(line, &CURSOR_COLOR, CURSOR_THICKNESS);

        // Draw the mark.
        if let Some(mark_time) = data.mark {
            let mark_x = PIXELS_PER_USEC * (mark_time as f64);
            let mut path = BezPath::new();
            path.move_to((mark_x - 8.0, 0.0));
            path.line_to((mark_x + 8.0, 0.0));
            path.line_to((mark_x, 8.0));
            path.close_path();
            ctx.fill(path, &MARK_COLOR);
        }
    }
}
