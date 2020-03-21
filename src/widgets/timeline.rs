use druid::kurbo::Line;
use druid::theme;
use druid::{
    BoxConstraints, Color, Env, Event, EventCtx, LayoutCtx, Lens, LensWrap, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetPod,
};
use std::collections::HashMap;

use crate::snippet::{LerpedCurve, Snippets, SnippetId};
use crate::ScribbleState;

const SNIPPET_HEIGHT: f64 = 20.0;
const NUM_SNIPPETS: f64 = 5.0;
const MIN_WIDTH: f64 = 100.0;
const PIXELS_PER_USEC: f64 = 20.0 / 1000000.0;
const TIMELINE_BG_COLOR: Color = Color::rgb8(0x66, 0x66, 0x66);
const CURSOR_COLOR: Color = Color::rgb8(0x10, 0x10, 0xaa);
const CURSOR_THICKNESS: f64 = 3.0;

const SNIPPET_COLOR: Color = Color::rgb8(0x99, 0x99, 0x22);
const SNIPPET_STROKE_COLOR: Color = Color::rgb8(0, 0, 0);
const SNIPPET_STROKE_THICKNESS: f64 = 1.0;

fn timeline_snippet_same(c: &LerpedCurve, d: &LerpedCurve) -> bool {
    c.lerp == d.lerp
}

#[derive(Default)]
pub struct Timeline {
    snippet_offsets: HashMap<SnippetId, usize>,
    children: HashMap<SnippetId, WidgetPod<ScribbleState, LensWrap<LerpedCurve, SnippetLens, TimelineSnippet>>>,
}

struct SnippetBounds {
    start_us: i64,
    end_us: i64,
    id: SnippetId,
}

impl SnippetBounds {
    fn new(data: (SnippetId, &LerpedCurve)) -> SnippetBounds {
        SnippetBounds {
            start_us: data.1.lerp.first(),
            end_us: data.1.lerp.last(),
            id: data.0,
        }
    }
}

impl Timeline {
    fn recalculate_snippet_offsets(&mut self, snippets: &Snippets) {
        let mut bounds: Vec<_> = snippets
            .iter()
            .map(SnippetBounds::new)
            .collect();
        bounds.sort_by_key(|b| b.start_us);

        let mut row_ends = vec![0i64; NUM_SNIPPETS as usize];
        self.snippet_offsets.clear();
        'bounds: for b in &bounds {
            for (row_idx, end) in row_ends.iter_mut().enumerate() {
                if *end == 0 || b.start_us > *end
                /* TODO: more padding */
                {
                    *end = b.end_us;
                    self.snippet_offsets.insert(b.id, row_idx);
                    continue 'bounds;
                }
            }
            panic!("Too many overlapping snippets");
        }

        self.children.clear();
        for b in &bounds {
            self.children.insert(
                b.id,
                WidgetPod::new(LensWrap::new(TimelineSnippet {}, SnippetLens(b.id)))
            );
        }
    }
}

#[derive(Default)]
struct TimelineSnippet {}

// TODO: need something better than an index
struct SnippetLens(pub SnippetId);

impl Lens<ScribbleState, LerpedCurve> for SnippetLens {
    fn with<V, F: FnOnce(&LerpedCurve) -> V>(&self, data: &ScribbleState, f: F) -> V {
        // FIXME: use snippet id, not index
        f(&data.snippets.snippet(self.0))
    }

    fn with_mut<V, F: FnOnce(&mut LerpedCurve) -> V>(&self, data: &mut ScribbleState, f: F) -> V {
        f(&mut data.snippets.snippet_mut(self.0))
    }
}

#[allow(unused_variables)]
impl Widget<LerpedCurve> for TimelineSnippet {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, state: &mut LerpedCurve, _env: &Env) {
        match event {
            Event::MouseDown(_) => {
                todo!()
            }
            Event::MouseUp(_) => {
                todo!()
            }
            _ => {}
        }
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_state: &LerpedCurve,
        _state: &LerpedCurve,
        _env: &Env,
    ) {
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _: &LifeCycle,
        _state: &LerpedCurve,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &LerpedCurve,
        _env: &Env,
    ) -> Size {
        let width = (data.lerp.last() - data.lerp.first()) as f64 * PIXELS_PER_USEC;
        let height = SNIPPET_HEIGHT;
        bc.constrain((width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &LerpedCurve, env: &Env) {
        let width = (data.lerp.last() - data.lerp.first()) as f64 * PIXELS_PER_USEC;
        let height = SNIPPET_HEIGHT;
        let rect = Rect::from_origin_size(Point::ZERO, (width, height))
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        ctx.stroke(&rect, &SNIPPET_STROKE_COLOR, SNIPPET_STROKE_THICKNESS);
        ctx.fill(&rect, &SNIPPET_COLOR);
    }
}

impl Widget<ScribbleState> for Timeline {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut ScribbleState, _env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_state: &ScribbleState,
        state: &ScribbleState,
        _env: &Env,
    ) {
        // TODO: do this better
        if state.snippets.snippets().curves().count() != self.children.len() {
            ctx.request_layout();
        }
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
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &ScribbleState,
        env: &Env,
    ) -> Size {
        self.recalculate_snippet_offsets(&data.snippets.snippets());

        for (&id, &offset) in &self.snippet_offsets {
            let x = (data.snippets.snippet(id).lerp.first() as f64) * PIXELS_PER_USEC;
            let y = offset as f64 * SNIPPET_HEIGHT;

            // FIXME: shouldn't we modify bc before recursing?
            let child = self.children.get_mut(&id).unwrap();
            let size = child.layout(ctx, bc, data, env);
            child.set_layout_rect(Rect::from_origin_size((x, y), size));
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
    }
}
