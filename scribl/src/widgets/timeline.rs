use druid::kurbo::{BezPath, Line, Vec2};
use druid::theme;
use druid::widget::{Controller, Scroll};
use druid::{
    Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetExt, WidgetPod,
};
use std::collections::HashMap;

use scribl_curves::{SnippetData, SnippetId, SnippetsData, Time, TimeDiff};

use crate::audio::{AudioSnippetData, AudioSnippetId, AudioSnippetsData};
use crate::cmd;
use crate::editor_state::{EditorState, MaybeSnippetId};
use crate::snippet_layout::{self, SnippetShape};

const SNIPPET_HEIGHT: f64 = 20.0;
const PIXELS_PER_USEC: f64 = 40.0 / 1000000.0;
const TIMELINE_BG_COLOR: Color = Color::rgb8(0x66, 0x66, 0x66);
const CURSOR_COLOR: Color = Color::rgb8(0x10, 0x10, 0xaa);
const CURSOR_THICKNESS: f64 = 3.0;

const AUDIO_SNIPPET_COLOR: Color = crate::UI_LIGHT_YELLOW;
const AUDIO_SNIPPET_SELECTED_COLOR: Color = crate::UI_LIGHT_YELLOW;
const SNIPPET_STROKE_COLOR: Color = Color::rgb8(0x00, 0x00, 0x00);
const SNIPPET_SELECTED_STROKE_COLOR: Color = Color::rgb8(0xff, 0xff, 0xff);
const SNIPPET_STROKE_THICKNESS: f64 = 1.0;
const SNIPPET_SELECTED_STROKE_THICKNESS: f64 = 3.0;
const SNIPPET_WAVEFORM_COLOR: Color = crate::UI_DARK_BLUE;

const MIN_TIMELINE_HEIGHT: f64 = 100.0;

/// We don't allow the cursor to get closer to the edge of the window than this (unless it's at the
/// very beginning). If the cursor gets closer than this, we scroll the timeline to get it within
/// the bounds.
const CURSOR_BOUNDARY_PADDING: TimeDiff = TimeDiff::from_micros(1_000_000);
/// When they drag the cursor into the boundary region, we scroll by at most this speed factor (as
/// a multiple of real-time).
const CURSOR_DRAG_SCROLL_SPEED: f64 = 32.0;

const LAYOUT_PARAMS: crate::snippet_layout::Parameters = crate::snippet_layout::Parameters {
    thick_height: 18.0,
    thin_height: 2.0,
    h_padding: 5.0,
    v_padding: 2.0,
    min_width: 20.0,
    overlap: 5.0,
    end_x: 3_600_000_000.0 * PIXELS_PER_USEC,
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
    fn to_maybe_id(&self) -> MaybeSnippetId {
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
    strokes: Vec<(Time, Color)>,
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
        let mut last_color: Option<Color> = None;
        for stroke in data.strokes() {
            if let Some(&t) = stroke.times.first() {
                if last_color.as_ref().map(|c| c.as_rgba()) != Some(stroke.style.color.as_rgba()) {
                    strokes.push((t, stroke.style.color.clone()));
                    last_color = Some(stroke.style.color.clone());
                }
            }
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
    /// The range of times that are currently visible. This needs to be manually synced with the
    /// scroll region's offset; this is handled by TimelineScrollController.
    visible_times: (Time, Time),
    height: f64,
    /// If the cursor is being dragged to near the edge of the timeline, this is how fast we should
    /// scroll in response.
    cursor_drag_scroll_speed: Option<f64>,
    children: HashMap<Id, WidgetPod<EditorState, TimelineSnippet>>,
}

pub fn make_timeline() -> impl Widget<EditorState> {
    let inner = TimelineInner::default();
    Scroll::new(inner)
        .horizontal()
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

impl Controller<EditorState, Scroll<EditorState, TimelineInner>> for TimelineScrollController {
    fn event(
        &mut self,
        child: &mut Scroll<EditorState, TimelineInner>,
        ctx: &mut EventCtx,
        ev: &Event,
        data: &mut EditorState,
        env: &Env,
    ) {
        child.event(ctx, ev, data, env);
        let offset = child.offset().x;
        child
            .child_mut()
            .set_visible(x_pix(offset), x_pix(offset + ctx.size().width));
    }

    fn update(
        &mut self,
        child: &mut Scroll<EditorState, TimelineInner>,
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
            let padding = CURSOR_BOUNDARY_PADDING.min(width_pix(size.width / 4.0));

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

        let offset = child.offset().x;
        child
            .child_mut()
            .set_visible(x_pix(offset), x_pix(offset + ctx.size().width));
    }

    fn lifecycle(
        &mut self,
        child: &mut Scroll<EditorState, TimelineInner>,
        ctx: &mut LifeCycleCtx,
        ev: &LifeCycle,
        data: &EditorState,
        env: &Env,
    ) {
        child.lifecycle(ctx, ev, data, env);
        let offset = child.offset().x;
        child
            .child_mut()
            .set_visible(x_pix(offset), x_pix(offset + ctx.size().width));
    }
}

impl Default for TimelineInner {
    fn default() -> TimelineInner {
        TimelineInner {
            visible_times: (Time::ZERO, Time::ZERO),
            height: MIN_TIMELINE_HEIGHT,
            cursor_drag_scroll_speed: None,
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

    fn fill_color(&self, data: &EditorState) -> Option<Color> {
        match self.id {
            Id::Drawing(_) => None,
            Id::Audio(id) => {
                if data.selected_snippet == id.into() {
                    Some(AUDIO_SNIPPET_SELECTED_COLOR)
                } else {
                    Some(AUDIO_SNIPPET_COLOR)
                }
            }
        }
    }

    fn path(&self) -> &BezPath {
        &self.path
    }

    fn contains(&self, p: Point) -> bool {
        self.shape.rects.iter().any(|r| r.contains(p))
    }

    /// If this snippet intersects the horizontal position `x`, returns the y interval
    /// of this snippet at that coordinate.
    fn y_interval(&self, x: f64) -> Option<(f64, f64)> {
        let mut min = f64::INFINITY;
        let mut max = -f64::INFINITY;

        for r in &self.shape.rects {
            if r.x0 <= x && x <= r.x1 {
                min = min.min(r.y0);
                max = max.max(r.y1);
            }
        }

        if min <= max {
            Some((min, max))
        } else {
            None
        }
    }

    /// Returns the y interval of this snippet at its closest point to `x`.
    fn closest_y_interval(&self, x: f64) -> (f64, f64) {
        if let Some(r) = self.shape.rects.first() {
            if x < r.x0 {
                return (r.y0, r.y1);
            }
        }
        if let Some(r) = self.shape.rects.last() {
            if x > r.x1 {
                return (r.y0, r.y1);
            }
        }
        if let Some(int) = self.y_interval(x) {
            return int;
        }
        dbg!(x, &self.shape.rects);
        log::error!("ran out of options in closest_y_interval");
        (0.0, 0.0)
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
                let mut start_x = 0.0;
                let mut last_color = &Color::BLACK;
                for &(start, ref color) in &segs.strokes {
                    let end_x = pix_width(start - data.start_time());
                    let rect = Rect::from_points((start_x, 0.0), (end_x, height));
                    ctx.fill(&rect, last_color);
                    last_color = color;
                    start_x = end_x;
                }

                let last_rect = Rect::from_points((start_x, 0.0), (LAYOUT_PARAMS.end_x, height));
                ctx.fill(&last_rect, last_color);

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
        let path = self.path().clone();
        let fill_color = self.fill_color(data);

        ctx.with_save(|ctx| {
            let clip = ctx.region().bounding_box();
            ctx.clip(clip);
            if let Some(c) = fill_color {
                ctx.fill(&path, &c);
            }
            ctx.with_save(|ctx| {
                ctx.clip(&path);
                ctx.transform(Affine::translate((pix_x(snippet.start_time()), 0.0)));
                self.render_interior(ctx, &snippet, height);
            });

            if is_selected {
                ctx.stroke(
                    &path,
                    &SNIPPET_SELECTED_STROKE_COLOR,
                    SNIPPET_SELECTED_STROKE_THICKNESS,
                );
            }
            if self.hot {
                ctx.stroke(&path, &SNIPPET_STROKE_COLOR, SNIPPET_STROKE_THICKNESS);
            }
        });
    }
}

impl TimelineInner {
    fn y_intervals<'a>(&'a self, x: f64) -> impl Iterator<Item = (Id, f64, f64)> + 'a {
        self.children.iter().filter_map(move |(&id, snip)| {
            if let Some((y0, y1)) = snip.widget().y_interval(x) {
                Some((id, y0, y1))
            } else {
                None
            }
        })
    }

    fn selected<'a>(&'a self, data: &EditorState) -> Option<&'a TimelineSnippet> {
        match data.selected_snippet {
            MaybeSnippetId::None => None,
            MaybeSnippetId::Draw(id) => self.children.get(&Id::Drawing(id)).map(|w| w.widget()),
            MaybeSnippetId::Audio(id) => self.children.get(&Id::Audio(id)).map(|w| w.widget()),
        }
    }

    fn set_visible(&mut self, start_time: Time, end_time: Time) {
        self.visible_times = (start_time, end_time);
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
                ctx.submit_command(cmd::WARP_TIME_TO.with(time));
                ctx.set_active(true);
            }
            Event::MouseMove(ev) => {
                // On click-and-drag, we change the time with the drag.
                if ctx.is_active() {
                    // If the mouse is near the boundary, we scroll smoothly instead of snapping to
                    // that position.
                    let time = Time::from_micros((ev.pos.x.max(0.0) / PIXELS_PER_USEC) as i64);
                    let factor =
                        CURSOR_DRAG_SCROLL_SPEED / CURSOR_BOUNDARY_PADDING.as_micros() as f64;
                    if time < self.visible_times.0 + CURSOR_BOUNDARY_PADDING {
                        if self.visible_times.0 > Time::ZERO {
                            let speed = (time - self.visible_times.0 - CURSOR_BOUNDARY_PADDING)
                                .as_micros() as f64
                                * factor;
                            self.cursor_drag_scroll_speed =
                                Some(speed.max(-CURSOR_DRAG_SCROLL_SPEED));
                            ctx.request_anim_frame();
                        } else {
                            self.cursor_drag_scroll_speed = None;
                        }
                    } else if time >= self.visible_times.1 - CURSOR_BOUNDARY_PADDING {
                        let speed = (time - self.visible_times.1 + CURSOR_BOUNDARY_PADDING)
                            .as_micros() as f64
                            * factor;
                        self.cursor_drag_scroll_speed = Some(speed.min(CURSOR_DRAG_SCROLL_SPEED));
                        ctx.request_anim_frame();
                    } else {
                        self.cursor_drag_scroll_speed = None;
                        ctx.submit_command(cmd::WARP_TIME_TO.with(time));
                    }
                }
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                }
                self.cursor_drag_scroll_speed = None;
            }
            Event::Command(c) => {
                let x = pix_x(data.time());
                let y_int = self.selected(data).map(|s| s.closest_y_interval(x));

                if c.is(cmd::SELECT_SNIPPET_ABOVE) {
                    ctx.set_handled();

                    let y = y_int.map(|int| int.0).unwrap_or(self.height);
                    let id = self
                        .y_intervals(x)
                        .filter(|&(_id, _y0, y1)| y1 <= y)
                        .max_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
                        .map(|a| a.0);
                    if let Some(id) = id {
                        data.selected_snippet = id.to_maybe_id();
                    }
                } else if c.is(cmd::SELECT_SNIPPET_BELOW) {
                    ctx.set_handled();

                    let y = y_int.map(|int| int.1).unwrap_or(0.0);
                    let id = self
                        .y_intervals(x)
                        .filter(|&(_id, y0, _y1)| y0 >= y)
                        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
                        .map(|a| a.0);
                    if let Some(id) = id {
                        data.selected_snippet = id.to_maybe_id();
                    }
                }
            }
            Event::AnimFrame(ns_elapsed) => {
                if let Some(speed) = self.cursor_drag_scroll_speed {
                    let time = data.time()
                        + TimeDiff::from_micros((speed * *ns_elapsed as f64 / 1000.0) as i64);
                    // Modify the data directly instead of with a command, because the command
                    // won't arrive until the next frame at the earliest.
                    if data.action.is_idle() {
                        data.warp_time_to(time);
                    }
                    ctx.request_anim_frame();
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
        // In principle the width could be infinite, but druid doesn't like that.
        let size = bc.constrain((10e7, self.height));

        // The children have funny shapes, so rather than use druid's layout mechanisms to position
        // them, we just do it all manually. Nevertheless, we need to call "layout" on the children
        // so that druid will know that we already laid them out.
        for c in self.children.values_mut() {
            c.layout(ctx, bc, data, env);
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
