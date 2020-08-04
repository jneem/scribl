use druid::kurbo::{BezPath, Line, Vec2};
use druid::theme;
use druid::widget::{Controller, Scroll};
use druid::{
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetExt,
    WidgetPod,
};
use std::collections::HashMap;

use scribl_curves::{SnippetData, SnippetId, SnippetsData, Time, TimeDiff};

use crate::audio::{AudioSnippetData, AudioSnippetId, AudioSnippetsData};
use crate::cmd;
use crate::editor_state::EditorState;
use crate::snippet_layout::{self, SnippetShape};

const SNIPPET_HEIGHT: f64 = 20.0;
const PIXELS_PER_USEC: f64 = 40.0 / 1000000.0;
const TIMELINE_BG_COLOR: Color = Color::rgb8(0x66, 0x66, 0x66);
const CURSOR_COLOR: Color = Color::rgb8(0x10, 0x10, 0xaa);
const CURSOR_THICKNESS: f64 = 3.0;

const DRAW_SNIPPET_COLOR: Color = crate::UI_BEIGE;
const DRAW_SNIPPET_SELECTED_COLOR: Color = crate::UI_LIGHT_STEEL_BLUE;
const AUDIO_SNIPPET_COLOR: Color = crate::UI_LIGHT_YELLOW;
const AUDIO_SNIPPET_SELECTED_COLOR: Color = crate::UI_LIGHT_GREEN;
const SNIPPET_STROKE_COLOR: Color = Color::rgb8(0x00, 0x00, 0x00);
const SNIPPET_STROKE_THICKNESS: f64 = 2.0;
const SNIPPET_WAVEFORM_COLOR: Color = crate::UI_DARK_BLUE;

const MIN_TIMELINE_HEIGHT: f64 = 100.0;
const LAYOUT_PARAMS: crate::snippet_layout::Parameters = crate::snippet_layout::Parameters {
    thick_height: 18.0,
    thin_height: 2.0,
    h_padding: 5.0,
    v_padding: 2.0,
    min_width: 20.0,
    overlap: 5.0,
    pixels_per_usec: PIXELS_PER_USEC,
};

const MARK_COLOR: Color = Color::rgb8(0x33, 0x33, 0x99);

/// Converts from a time interval to a width in pixels.
fn pix_width(d: TimeDiff) -> f64 {
    d.as_micros() as f64 * PIXELS_PER_USEC
}

/// Converts from a width in pixels to a time interval.
fn width_pix(p: f64) -> TimeDiff {
    TimeDiff::from_micros((p / PIXELS_PER_USEC) as i64)
}

/// Converts from a time instant to an x-position in pixels.
fn pix_x(t: Time) -> f64 {
    t.as_micros() as f64 * PIXELS_PER_USEC
}

/// Converts from an x-position in pixels to a time instant.
fn x_pix(p: f64) -> Time {
    Time::from_micros((p / PIXELS_PER_USEC) as i64)
}

/// The id of a snippet (either a drawing snippet or an audio snippet).
// TODO: do we need both this and MaybeSnippetId?
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Id {
    Drawing(SnippetId),
    Audio(AudioSnippetId),
}

impl Id {
    fn to_maybe_id(&self) -> crate::editor_state::MaybeSnippetId {
        use crate::editor_state::MaybeSnippetId;
        match self {
            Id::Drawing(id) => MaybeSnippetId::Draw(*id),
            Id::Audio(id) => MaybeSnippetId::Audio(*id),
        }
    }
}

/// The cached "waveform" of an audio snippet.
struct AudioWaveform {
    // The shape of the waveform.
    wave: BezPath,
}

/// The cached "waveform" of a drawing snippet.
struct DrawingWaveform {
    strokes: Vec<(Time, Time, Color)>,
}

enum SnippetInterior {
    Audio(AudioWaveform),
    Drawing(DrawingWaveform),
}

/// The data of a snippet (either a drawing snippet or an audio snippet).
#[derive(Clone, Data)]
enum Snip {
    Drawing(SnippetData),
    Audio(AudioSnippetData),
}

impl AudioWaveform {
    fn new(data: AudioSnippetData, shape: &crate::snippet_layout::SnippetShape) -> AudioWaveform {
        if shape.rects.is_empty() {
            return AudioWaveform {
                wave: BezPath::new(),
            };
        }

        // Converts a PCM sample to a y coordinate. This could use some more
        // thought and/or testing. Like, should we be taking a logarithm somewhere?
        let audio_height = |x: f64| -> f64 {
            (x * (data.multiplier() as f64) / std::i16::MAX as f64)
                .max(-1.0)
                .min(1.0)
        };

        let pix_per_sample = 5;
        let buf = data.buf();
        let mut path_back = Vec::new();
        let mut path = BezPath::new();
        let x0 = shape.rects[0].x0;
        path.move_to((0.0, shape.rects[0].center().y));
        for (i, r) in shape.rects.iter().enumerate() {
            let start = if i == 0 {
                0
            } else {
                (LAYOUT_PARAMS.overlap / 2.0) as usize
            };
            let width = if i + 1 == shape.rects.len() {
                r.width() as usize
            } else {
                (r.width() - LAYOUT_PARAMS.overlap / 2.0) as usize
            };
            for p in (start..width).step_by(pix_per_sample) {
                let start_time = x_pix(p as f64 + r.x0 - x0) - Time::ZERO;
                let end_time = x_pix((p + pix_per_sample) as f64 + r.x0 - x0) - Time::ZERO;
                let start_idx =
                    (start_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize).min(buf.len());
                let end_idx =
                    (end_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize).min(buf.len());
                let sub_buf = &buf[start_idx..end_idx];

                let mag = (sub_buf.iter().cloned().max().unwrap_or(0) as f64
                    - sub_buf.iter().cloned().min().unwrap_or(0) as f64)
                    / 2.0;

                let x = p as f64 + r.x0 - x0;
                let dy = audio_height(mag) / 2.0 * r.height();
                path.line_to((x, r.center().y + dy));
                path_back.push((x, r.center().y - dy));
            }
        }

        for (x, y) in path_back.into_iter().rev() {
            path.line_to((x, y));
        }
        path.close_path();
        AudioWaveform { wave: path }
    }
}

impl DrawingWaveform {
    fn new(data: &SnippetData) -> DrawingWaveform {
        let mut strokes = Vec::new();
        for stroke in data.strokes() {
            if stroke.times.is_empty() {
                continue;
            }
            strokes.push((
                *stroke.times.first().unwrap(),
                *stroke.times.last().unwrap(),
                stroke.style.color,
            ));
        }
        DrawingWaveform { strokes }
    }
}

impl Snip {
    /// At what time does this snippet start?
    fn start_time(&self) -> Time {
        match self {
            Snip::Audio(s) => s.start_time(),
            Snip::Drawing(d) => d.start_time(),
        }
    }

    /// At what time does this snippet end? Returns `None` if the snippet never
    /// ends.
    fn end_time(&self) -> Option<Time> {
        match self {
            Snip::Audio(s) => Some(s.end_time()),
            Snip::Drawing(d) => d.end_time(),
        }
    }

    /// Returns the list of times at which this snippet was lerped.
    fn inner_lerp_times(&self) -> Vec<TimeDiff> {
        match self {
            Snip::Audio(_) => Vec::new(),
            Snip::Drawing(d) => {
                let lerps = d.key_times();
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
}

/// The main timeline widget.
struct TimelineInner {
    height: f64,
    children: HashMap<Id, WidgetPod<EditorState, TimelineSnippet>>,
}

pub fn make_timeline() -> impl Widget<EditorState> {
    let inner = TimelineInner::default();
    Scroll::new(inner)
        .controller(TimelineScrollController)
        // This is a hack to hide the scrollbars. Hopefully in the future druid will
        // support this directly.
        .env_scope(|env, _data| {
            env.set(theme::SCROLLBAR_WIDTH, 0.0);
            env.set(theme::SCROLLBAR_EDGE_WIDTH, 0.0);
        })
}

/// A widget wrapping the timeline's `Scroll` that updates the scroll to follow
/// the cursor.
struct TimelineScrollController;

impl<W: Widget<EditorState>> Controller<EditorState, Scroll<EditorState, W>>
    for TimelineScrollController
{
    fn update(
        &mut self,
        child: &mut Scroll<EditorState, W>,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if data.time() != old_data.time() {
            // Scroll the cursor to the new time.
            let time = data.time();
            let size = ctx.size();
            let min_vis_time = x_pix(child.offset().x);
            let max_vis_time = x_pix(child.offset().x + size.width);

            // Scroll this much past the cursor, so it isn't right at the edge.
            let padding = TimeDiff::from_micros(1_000_000).min(width_pix(size.width / 4.0));

            let delta_x = if time + padding > max_vis_time {
                pix_width(time - max_vis_time + padding)
            } else if time - padding < min_vis_time {
                pix_width(time - min_vis_time - padding)
            } else {
                0.0
            };

            if delta_x != 0.0 {
                ctx.request_paint();
            }
            child.scroll(Vec2 { x: delta_x, y: 0.0 }, size);
        }
        child.update(ctx, old_data, data, env);
    }
}

impl Default for TimelineInner {
    fn default() -> TimelineInner {
        TimelineInner {
            height: MIN_TIMELINE_HEIGHT,
            children: HashMap::new(),
        }
    }
}

impl TimelineInner {
    // Recreates the child widgets, and organizes them into rows so that they don't overlap.
    fn recreate_children(&mut self, snippets: &SnippetsData, audio: &AudioSnippetsData) {
        let draw_shapes = snippet_layout::layout(snippets.snippets(), &LAYOUT_PARAMS);
        let audio_shapes = snippet_layout::layout(audio.snippets(), &LAYOUT_PARAMS);
        self.height = (draw_shapes.max_y + audio_shapes.max_y).max(MIN_TIMELINE_HEIGHT);

        self.children.clear();
        for (id, shape) in draw_shapes.positions {
            let snip = snippets.snippet(id);
            let id = Id::Drawing(id);
            let interior = SnippetInterior::Drawing(DrawingWaveform::new(&snip));
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    path: shape.to_path(LAYOUT_PARAMS.overlap),
                    hot: false,
                    shape,
                    interior,
                }),
            );
        }
        for (id, mut shape) in audio_shapes.positions {
            shape.reflect_y(self.height);
            let audio_data = audio.snippet(id);
            let id = Id::Audio(id);
            let interior = SnippetInterior::Audio(AudioWaveform::new(audio_data.clone(), &shape));
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    path: shape.to_path(LAYOUT_PARAMS.overlap),
                    hot: false,
                    shape: shape.clone(),
                    interior,
                }),
            );
        }
    }

    fn invalid_rect(s: Time, t: Time, height: f64) -> Rect {
        let x1 = pix_x(s);
        let x2 = pix_x(t);
        Rect::from_points((x1, 0.0), (x2, height))
            .inset((CURSOR_THICKNESS / 2.0, 0.0))
            .expand()
    }
}

/// A widget representing a single snippet (audio or drawing) in the timeline.
struct TimelineSnippet {
    // The id of the snippet that this widget represents.
    id: Id,
    // Because a timeline snippet isn't rectangle-shaped, we do our own hot-state tracking.
    hot: bool,
    path: BezPath,
    shape: SnippetShape,
    interior: SnippetInterior,
}

impl TimelineSnippet {
    fn snip(&self, data: &EditorState) -> Snip {
        match self.id {
            Id::Drawing(id) => Snip::Drawing(data.snippets.snippet(id).clone()),
            Id::Audio(id) => Snip::Audio(data.audio_snippets.snippet(id).clone()),
        }
    }

    fn width(&self, data: &EditorState) -> f64 {
        let snip = self.snip(data);
        if let Some(end_time) = snip.end_time() {
            pix_width(end_time - snip.start_time())
        } else {
            std::f64::INFINITY
        }
    }

    fn fill_color(&self, data: &EditorState) -> Color {
        match self.id {
            Id::Drawing(id) => {
                if data.selected_snippet == id.into() {
                    DRAW_SNIPPET_SELECTED_COLOR
                } else {
                    DRAW_SNIPPET_COLOR
                }
            }
            Id::Audio(id) => {
                if data.selected_snippet == id.into() {
                    AUDIO_SNIPPET_SELECTED_COLOR
                } else {
                    AUDIO_SNIPPET_COLOR
                }
            }
        }
    }

    fn stroke_color(&self) -> Color {
        match self.id {
            Id::Drawing(_) => DRAW_SNIPPET_SELECTED_COLOR,
            Id::Audio(_) => AUDIO_SNIPPET_SELECTED_COLOR,
        }
    }

    fn path(&self) -> &BezPath {
        &self.path
    }

    fn contains(&self, p: Point) -> bool {
        self.shape.rects.iter().any(|r| r.contains(p))
    }

    /// Draws the "interior" of the snippet (i.e., everything but the bounding rect).
    fn render_interior(&self, ctx: &mut PaintCtx, snip: &Snip, height: f64) {
        match snip {
            Snip::Audio(_data) => {
                ctx.with_save(|ctx| match &self.interior {
                    SnippetInterior::Audio(a) => {
                        ctx.fill(&a.wave, &SNIPPET_WAVEFORM_COLOR);
                    }
                    _ => panic!("audio snippet should have a cached waveform"),
                });
            }
            Snip::Drawing(data) => {
                let segs = match &self.interior {
                    SnippetInterior::Drawing(s) => s,
                    _ => panic!("drawing widget should have cached segment extents"),
                };
                for &(start, end, ref color) in &segs.strokes {
                    let start_x = pix_width(start - data.start_time());
                    let end_x = pix_width(end - data.start_time());
                    let rect = Rect::from_points((start_x, 0.0), (end_x, height));
                    ctx.fill(&rect, color);
                }

                // Draw the lerp lines.
                for t in snip.inner_lerp_times() {
                    let x = pix_width(t);
                    ctx.stroke(Line::new((x, 0.0), (x, height)), &SNIPPET_STROKE_COLOR, 1.0);
                }
            }
        }
    }
}

impl Widget<EditorState> for TimelineSnippet {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut EditorState, _env: &Env) {
        match event {
            Event::MouseDown(ev) if ev.button.is_left() && self.contains(ev.pos) => {
                ctx.set_active(true);
                ctx.set_handled();
            }
            Event::MouseUp(ev) if ev.button.is_left() && self.contains(ev.pos) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    if self.hot {
                        match self.id {
                            Id::Drawing(id) => data.selected_snippet = id.into(),
                            Id::Audio(id) => data.selected_snippet = id.into(),
                        }
                        ctx.set_handled();
                        ctx.set_menu(crate::menus::make_menu(data));
                    }
                }
            }
            Event::MouseMove(ev) => {
                let new_hot = self.contains(ev.pos);
                if self.hot != new_hot {
                    self.hot = new_hot;
                    ctx.request_paint();
                }
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
        let snip = self.snip(data);
        let old_snip = self.snip(old_data);
        if !snip.same(&old_snip) {
            ctx.request_layout();
        }

        if old_data.selected_snippet != data.selected_snippet {
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _event: &LifeCycle,
        _data: &EditorState,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        _env: &Env,
    ) -> Size {
        let width = self.width(data);
        let height = SNIPPET_HEIGHT;
        bc.constrain((width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, _env: &Env) {
        let snippet = self.snip(data);
        let height = ctx.size().height;
        let is_selected = data.selected_snippet == self.id.to_maybe_id();

        let stroke_thick = if self.hot && !is_selected {
            SNIPPET_STROKE_THICKNESS
        } else {
            0.0
        };
        let path = self.path().clone();
        let stroke_color = self.stroke_color();
        let fill_color = self.fill_color(data);

        ctx.with_save(|ctx| {
            let clip = ctx.region().bounding_box();
            ctx.clip(clip);
            ctx.fill(&path, &fill_color);
            ctx.clip(&path);
            ctx.with_save(|ctx| {
                ctx.transform(Affine::translate((pix_x(snippet.start_time()), 0.0)));
                self.render_interior(ctx, &snippet, height);
            });
            if stroke_thick > 0.0 {
                ctx.stroke(&path, &stroke_color, stroke_thick);
            }
        });
    }
}

impl Widget<EditorState> for TimelineInner {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut EditorState, env: &Env) {
        match event {
            Event::WindowConnected => {
                ctx.request_paint();
            }
            Event::MouseDown(ev) => {
                let time = Time::from_micros((ev.pos.x / PIXELS_PER_USEC) as i64);
                ctx.submit_command(Command::new(cmd::WARP_TIME_TO, time), None);
                ctx.set_active(true);
            }
            Event::MouseMove(ev) => {
                // On click-and-drag, we change the time with the drag.
                if ctx.is_active() {
                    let time = Time::from_micros((ev.pos.x.max(0.0) / PIXELS_PER_USEC) as i64);
                    ctx.submit_command(Command::new(cmd::WARP_TIME_TO, time), None);
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
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if !data.snippets.same(&old_data.snippets)
            || !data.audio_snippets.same(&old_data.audio_snippets)
        {
            ctx.request_layout();
            self.recreate_children(&data.snippets, &data.audio_snippets);
            ctx.children_changed();
        }
        if old_data.mark != data.mark {
            ctx.request_paint();
        }
        if old_data.time() != data.time() {
            let invalid =
                TimelineInner::invalid_rect(old_data.time(), data.time(), ctx.size().height);
            ctx.request_paint_rect(invalid);
        }
        for child in self.children.values_mut() {
            child.update(ctx, data, env);
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &EditorState,
        env: &Env,
    ) {
        match event {
            LifeCycle::WidgetAdded => {
                ctx.request_layout();
                self.recreate_children(&data.snippets, &data.audio_snippets);
                ctx.children_changed();
            }
            _ => {}
        }
        for child in self.children.values_mut() {
            child.lifecycle(ctx, event, data, env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        env: &Env,
    ) -> Size {
        let size = bc.constrain((std::f64::INFINITY, self.height));

        // The children have funny shapes, so rather than use druid's layout mechanisms to position
        // them, we just do it all manually.
        for c in self.children.values_mut() {
            c.set_layout_rect(ctx, data, env, size.to_rect());
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        // Note that the width here may well be infinite. Intersecting with the
        // paint region will prevent us from trying to fill an infinite rect.
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size).intersect(ctx.region().bounding_box());
        ctx.fill(rect, &TIMELINE_BG_COLOR);

        for child in self.children.values_mut() {
            child.paint(ctx, data, env);
        }

        // Draw the cursor.
        let cursor_x = pix_x(data.time());
        let line = Line::new((cursor_x, 0.0), (cursor_x, size.height));
        ctx.stroke(line, &CURSOR_COLOR, CURSOR_THICKNESS);

        // Draw the mark.
        if let Some(mark_time) = data.mark {
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
