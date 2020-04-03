use druid::kurbo::{BezPath, Line};
use druid::theme;
use druid::{
    BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetPod,
};
use std::collections::HashMap;

use crate::audio::{AudioSnippetData, AudioSnippetId, AudioSnippetsData};
use crate::data::{AppState, SnippetData, SnippetsData};
use crate::snippet::SnippetId;
use crate::snippet_layout;
use crate::time::{self, Diff, Time};

const SNIPPET_HEIGHT: f64 = 20.0;
const MIN_NUM_ROWS: usize = 5;
const MIN_WIDTH: f64 = 100.0;
const PIXELS_PER_USEC: f64 = 100.0 / 1000000.0;
const TIMELINE_BG_COLOR: Color = Color::rgb8(0x66, 0x66, 0x66);
const CURSOR_COLOR: Color = Color::rgb8(0x10, 0x10, 0xaa);
const CURSOR_THICKNESS: f64 = 3.0;

const DRAW_SNIPPET_COLOR: Color = Color::rgb8(0x99, 0x99, 0x22);
const DRAW_SNIPPET_SELECTED_COLOR: Color = Color::rgb8(0x77, 0x77, 0x11);
const AUDIO_SNIPPET_COLOR: Color = Color::rgb8(0x55, 0x55, 0xBB);
const SNIPPET_STROKE_COLOR: Color = Color::rgb8(0x22, 0x22, 0x22);
const SNIPPET_HOVER_STROKE_COLOR: Color = Color::rgb8(0, 0, 0);
const SNIPPET_STROKE_THICKNESS: f64 = 1.0;
const SNIPPET_WAVEFORM_COLOR: Color = Color::rgb8(0x33, 0x33, 0x99);

const MARK_COLOR: Color = Color::rgb8(0x33, 0x33, 0x99);

fn pix_width(d: Diff) -> f64 {
    d.as_micros() as f64 * PIXELS_PER_USEC
}

fn pix_x(t: Time) -> f64 {
    t.as_micros() as f64 * PIXELS_PER_USEC
}

fn x_pix(p: f64) -> Time {
    Time::from_micros((p / PIXELS_PER_USEC) as i64)
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Id {
    Drawing(SnippetId),
    Audio(AudioSnippetId),
}

#[derive(Clone, Data)]
enum Snip {
    Drawing(SnippetData),
    Audio(AudioSnippetData),
}

impl Snip {
    fn start_time(&self) -> Time {
        match self {
            Snip::Audio(s) => s.start_time(),
            Snip::Drawing(d) => d.start_time(),
        }
    }

    fn end_time(&self) -> Option<Time> {
        match self {
            Snip::Audio(s) => Some(s.end_time()),
            Snip::Drawing(d) => d.end_time(),
        }
    }

    fn last_draw_time(&self) -> Option<Time> {
        match self {
            Snip::Audio(_) => None,
            Snip::Drawing(d) => {
                let end = d.end_time().unwrap_or(Time::from_micros(std::i64::MAX));
                Some(d.last_draw_time().min(end))
            }
        }
    }

    fn inner_lerp_times(&self) -> Vec<Diff> {
        match self {
            Snip::Audio(_) => Vec::new(),
            Snip::Drawing(d) => {
                let lerps = d.lerp.times();
                let first_idx = lerps
                    .iter()
                    .position(|&x| x != lerps[0])
                    .unwrap_or(lerps.len());
                let last_idx = lerps
                    .iter()
                    .rposition(|&x| x != lerps[lerps.len() - 1])
                    .unwrap_or(0);
                if first_idx <= last_idx {
                    lerps[first_idx..=last_idx]
                        .iter()
                        .map(|&x| x - lerps[0])
                        .collect()
                } else {
                    Vec::new()
                }
            }
        }
    }

    fn render_interior(&self, ctx: &mut PaintCtx, width: f64, height: f64) {
        match self {
            Snip::Audio(data) => {
                // Converts a PCM sample to a y coordinate.
                let audio_height = |x: i16| -> f64 {
                    let sign = x.signum();
                    let x = x.abs() as f64 + 1.0;

                    // This gives a height between -1 and 1.
                    let y = sign as f64 * x.log(std::i16::MAX as f64);
                    // Now convert it to graphical y coord.
                    height / 2.0 + height * y / 2.0
                };

                let pix_per_sample = 5;
                let buf = data.buf();
                // TODO: shouldn't be recalculating this every paint.
                let mut mins = Vec::with_capacity((width as usize) / pix_per_sample);
                let mut path = BezPath::new();
                path.move_to((0.0, 0.0));
                for p in (0..(width as usize)).step_by(pix_per_sample) {
                    let start_time = x_pix(p as f64) - time::ZERO;
                    let end_time = x_pix((p + pix_per_sample) as f64) - time::ZERO;
                    let start_idx = (start_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize)
                        .min(buf.len());
                    let end_idx =
                        (end_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize).min(buf.len());
                    let sub_buf = &buf[start_idx..end_idx];

                    let max = sub_buf.iter().cloned().max().unwrap_or(0);
                    path.line_to((p as f64, audio_height(max)));
                    mins.push((p, sub_buf.iter().cloned().min().unwrap_or(0)));
                }

                for (p, min) in mins.into_iter().rev() {
                    path.line_to((p as f64, audio_height(min)));
                }
                path.close_path();
                ctx.fill(path, &SNIPPET_WAVEFORM_COLOR);
            }
            Snip::Drawing(data) => {
                // Draw the span of the edited region.
                let end = data.end_time().unwrap_or(Time::from_micros(std::i64::MAX));
                let last_draw_time = data.last_draw_time().min(end);
                let draw_width = pix_width(last_draw_time - data.start_time());
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

                // Draw the lerp lines.
                for t in self.inner_lerp_times() {
                    let x = pix_width(t);
                    ctx.stroke(Line::new((x, 0.0), (x, height)), &SNIPPET_STROKE_COLOR, 1.0);
                }
            }
        }
    }
}

pub struct Timeline {
    snippet_offsets: HashMap<Id, usize>,
    num_rows: usize,
    children: HashMap<Id, WidgetPod<AppState, TimelineSnippet>>,
}

impl Default for Timeline {
    fn default() -> Timeline {
        Timeline {
            snippet_offsets: HashMap::new(),
            num_rows: MIN_NUM_ROWS,
            children: HashMap::new(),
        }
    }
}

impl Timeline {
    fn recalculate_snippet_offsets(&mut self, snippets: &SnippetsData, audio: &AudioSnippetsData) {
        let draw_offsets = snippet_layout::layout(snippets.snippets());
        let audio_offsets = snippet_layout::layout(audio.snippets());
        self.num_rows = (draw_offsets.num_rows + audio_offsets.num_rows).max(MIN_NUM_ROWS);

        self.snippet_offsets.clear();
        self.children.clear();
        for (&id, &offset) in &draw_offsets.positions {
            let id = Id::Drawing(id);
            self.snippet_offsets.insert(id, offset);
            self.children
                .insert(id, WidgetPod::new(TimelineSnippet { id }));
        }
        for (&id, &offset) in &audio_offsets.positions {
            let id = Id::Audio(id);
            self.snippet_offsets.insert(id, self.num_rows - offset - 1);
            self.children
                .insert(id, WidgetPod::new(TimelineSnippet { id }));
        }
    }
}

struct TimelineSnippet {
    id: Id,
}

impl TimelineSnippet {
    fn snip(&self, data: &AppState) -> Snip {
        match self.id {
            Id::Drawing(id) => Snip::Drawing(data.scribble.snippets.snippet(id).clone()),
            Id::Audio(id) => Snip::Audio(data.scribble.audio_snippets.snippet(id).clone()),
        }
    }

    fn width(&self, data: &AppState) -> f64 {
        let snip = self.snip(data);
        if let Some(end_time) = snip.end_time() {
            pix_width(end_time - snip.start_time())
        } else {
            std::f64::INFINITY
        }
    }

    fn fill_color(&self, data: &AppState) -> Color {
        match self.id {
            Id::Drawing(id) => {
                if data.scribble.selected_snippet == Some(id) {
                    DRAW_SNIPPET_SELECTED_COLOR
                } else {
                    DRAW_SNIPPET_COLOR
                }
            }
            Id::Audio(_) => AUDIO_SNIPPET_COLOR,
        }
    }
}

impl Widget<AppState> for TimelineSnippet {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, _env: &Env) {
        match event {
            Event::MouseDown(ev) if ev.button.is_left() => {
                ctx.set_active(true);
                ctx.set_handled();
            }
            Event::MouseUp(ev) if ev.button.is_left() => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    if ctx.is_hot() {
                        if let Id::Drawing(id) = self.id {
                            data.scribble.selected_snippet = Some(id);
                            ctx.request_paint();
                            ctx.set_handled();
                        }
                    }
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, _env: &Env) {
        let snip = self.snip(data);
        let old_snip = self.snip(old_data);
        if !snip.same(&old_snip) {
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &AppState,
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
        data: &AppState,
        _env: &Env,
    ) -> Size {
        let width = self.width(data);
        let height = SNIPPET_HEIGHT;
        bc.constrain((width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, env: &Env) {
        let snippet = self.snip(data);
        let width = self.width(data).min(10000.0); // FIXME: there are bugs drawing infinite rects.
        let height = SNIPPET_HEIGHT;
        let rect = Rect::from_origin_size(Point::ZERO, (width, height))
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let stroke_color = if ctx.is_hot() {
            &SNIPPET_STROKE_COLOR
        } else {
            &SNIPPET_HOVER_STROKE_COLOR
        };
        let fill_color = self.fill_color(data);

        ctx.fill(&rect, &fill_color);
        ctx.stroke(&rect, stroke_color, SNIPPET_STROKE_THICKNESS);

        snippet.render_interior(ctx, width, height);
    }
}

impl Widget<AppState> for Timeline {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut AppState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_paint();
            }
            Event::MouseDown(ev) => {
                data.time = Time::from_micros((ev.pos.x / PIXELS_PER_USEC) as i64);
                ctx.set_active(true);
                ctx.request_paint();
            }
            Event::MouseMoved(ev) => {
                // On click-and-drag, we change the time with the drag.
                if ctx.is_active() {
                    data.time = Time::from_micros((ev.pos.x / PIXELS_PER_USEC) as i64);
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

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &AppState, data: &AppState, env: &Env) {
        if !data.scribble.snippets.same(&old_data.scribble.snippets)
            || !data
                .scribble
                .audio_snippets
                .same(&old_data.scribble.audio_snippets)
        {
            ctx.request_layout();
            self.recalculate_snippet_offsets(
                &data.scribble.snippets,
                &data.scribble.audio_snippets,
            );
            ctx.children_changed();
        }
        if old_data.time != data.time || old_data.scribble.mark != data.scribble.mark {
            ctx.request_paint();
        }
        for child in self.children.values_mut() {
            child.update(ctx, data, env);
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &AppState, env: &Env) {
        for child in self.children.values_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &AppState,
        env: &Env,
    ) -> Size {
        for (&id, &offset) in &self.snippet_offsets {
            let child = self.children.get_mut(&id).unwrap();
            let x = pix_x(child.widget().snip(data).start_time());
            let y = offset as f64 * SNIPPET_HEIGHT;

            let size = child.layout(ctx, bc, data, env);
            child.set_layout_rect(Rect::from_origin_size((x, y), size));
        }

        let height = SNIPPET_HEIGHT * self.num_rows as f64;
        bc.constrain((std::f64::INFINITY, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &AppState, env: &Env) {
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size);
        ctx.fill(rect, &TIMELINE_BG_COLOR);

        for child in self.children.values_mut() {
            child.paint_with_offset(ctx, data, env);
        }

        // Draw the cursor.
        let cursor_x = pix_x(data.time);
        let line = Line::new((cursor_x, 0.0), (cursor_x, size.height));
        ctx.stroke(line, &CURSOR_COLOR, CURSOR_THICKNESS);

        // Draw the mark.
        if let Some(mark_time) = data.scribble.mark {
            let mark_x = pix_x(mark_time);
            let mut path = BezPath::new();
            path.move_to((mark_x - 8.0, 0.0));
            path.line_to((mark_x + 8.0, 0.0));
            path.line_to((mark_x, 8.0));
            path.close_path();
            ctx.fill(path, &MARK_COLOR);
        }
    }
}
