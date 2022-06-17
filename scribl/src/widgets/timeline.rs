use druid::kurbo::{BezPath, Line, Shape, Vec2};
use druid::piet::StrokeStyle;
use druid::widget::ClipBox;
use druid::{
    Affine, BoxConstraints, Color, Data, Env, Event, EventCtx, KbKey, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetPod,
};
use std::collections::HashMap;

use scribl_curves::{DrawSnippet, DrawSnippets, Time, TimeDiff};
use scribl_widget::SunkenContainer;

use crate::audio::{TalkSnippet, TalkSnippets};
use crate::snippet_layout::{self, SnippetShape};
use crate::{cmd, EditorState, SnippetId};

const PIXELS_PER_USEC: f64 = 40.0 / 1000000.0;
const CURSOR_THICKNESS: f64 = 2.0;
const SELECTION_FILL_COLOR: Color = Color::rgba8(0xff, 0xff, 0xff, 0x20);

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
    h_padding: 2.0,
    v_padding: 2.0,
    min_width: 10.0,
    overlap: 5.0,
    end_x: 3_600_000_000.0 * PIXELS_PER_USEC,
    pixels_per_usec: PIXELS_PER_USEC,
};

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
    Drawing(DrawSnippet),
    Audio(TalkSnippet),
}

impl AudioWaveform {
    fn new(data: TalkSnippet, shape: &crate::snippet_layout::SnippetShape) -> AudioWaveform {
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
    fn new(data: &DrawSnippet) -> DrawingWaveform {
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

/// The "outer" timeline widget.
///
/// This basically just contains the scroll area, which contains the inner widget.
pub struct Timeline {
    inner:
        WidgetPod<EditorState, SunkenContainer<EditorState, ClipBox<EditorState, TimelineInner>>>,
}

/// The main timeline widget.
struct TimelineInner {
    /// The range of times that are currently visible. This needs to be manually synced with the
    /// scroll region's offset; this is handled by the outer Timeline widget.
    visible_times: (Time, Time),
    height: f64,
    /// If the cursor is being dragged to near the edge of the timeline, this is how fast we should
    /// scroll in response.
    cursor_drag_scroll_speed: Option<f64>,
    children: HashMap<SnippetId, WidgetPod<EditorState, TimelineSnippet>>,
}

impl Timeline {
    pub fn new() -> Timeline {
        let inner = TimelineInner::default();
        let clip = ClipBox::managed(inner)
            .constrain_horizontal(false)
            .constrain_vertical(true);
        Timeline {
            inner: WidgetPod::new(SunkenContainer::new(clip)),
        }
    }

    fn clip_box(&self) -> &ClipBox<EditorState, TimelineInner> {
        self.inner.widget().child()
    }

    fn clip_box_mut(&mut self) -> &mut ClipBox<EditorState, TimelineInner> {
        self.inner.widget_mut().child_mut()
    }

    // TODO: druid now has a mechanism for this (SCROLL_TO_VIEW). Use it.
    fn update_visible_times(&mut self, size: Size) {
        let offset = self.clip_box().viewport_origin().x;
        self.clip_box_mut()
            .child_mut()
            .set_visible(x_pix(offset), x_pix(offset + size.width));
    }
}

impl Widget<EditorState> for Timeline {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut EditorState, env: &Env) {
        if let Event::Wheel(wheel_ev) = ev {
            let delta = Vec2::new(wheel_ev.wheel_delta.x, 0.0);
            self.clip_box_mut().pan_by(delta);
            ctx.request_paint();
            ctx.set_handled();
        }
        self.inner.event(ctx, ev, data, env);
        self.update_visible_times(ctx.size());
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if data.time() != old_data.time() {
            // Scroll the cursor to the new time.
            let time = data.time();
            let size = ctx.size();
            let child = self.clip_box();
            let min_vis_time = x_pix(child.viewport_origin().x);
            let max_vis_time = x_pix(child.viewport_origin().x + size.width);

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
                self.clip_box_mut().pan_by(Vec2 { x: delta_x, y: 0.0 });
                ctx.request_paint();
            }
        }
        self.inner.update(ctx, data, env);
        self.update_visible_times(ctx.size());
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &EditorState, env: &Env) {
        self.inner.lifecycle(ctx, ev, data, env);
        self.update_visible_times(ctx.size());
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        env: &Env,
    ) -> Size {
        let child_size = self.inner.layout(ctx, bc, data, env);
        self.inner
            .set_layout_rect(ctx, data, env, child_size.to_rect());
        child_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        self.inner.paint(ctx, data, env);
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
    fn recreate_children(&mut self, snippets: &DrawSnippets, audio: &TalkSnippets) {
        let draw_shapes = snippet_layout::layout(snippets.snippets(), &LAYOUT_PARAMS);
        let audio_shapes = snippet_layout::layout(audio.snippets(), &LAYOUT_PARAMS);
        self.height = (draw_shapes.max_y + audio_shapes.max_y).max(MIN_TIMELINE_HEIGHT);

        self.children.clear();
        for (id, shape) in draw_shapes.positions {
            let snip = snippets.snippet(id);
            let id = SnippetId::Draw(id);
            let interior = SnippetInterior::Drawing(DrawingWaveform::new(&snip));
            let path = shape.to_path(LAYOUT_PARAMS.overlap);
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    bbox: path.bounding_box(),
                    path,
                    hot: false,
                    drag_start: None,
                    drag_shift: None,
                    shape,
                    interior,
                }),
            );
        }
        for (id, mut shape) in audio_shapes.positions {
            shape.reflect_y(self.height);
            let audio_data = audio.snippet(id);
            let id = SnippetId::Talk(id);
            let interior = SnippetInterior::Audio(AudioWaveform::new(audio_data.clone(), &shape));
            let path = shape.to_path(LAYOUT_PARAMS.overlap);
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    bbox: path.bounding_box(),
                    path,
                    hot: false,
                    drag_start: None,
                    drag_shift: None,
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
    id: SnippetId,
    // Because a timeline snippet isn't rectangle-shaped, we do our own hot-state tracking.
    hot: bool,
    // If they're dragging the snippet, this is the mouse position when they started.
    drag_start: Option<Time>,
    // If they're dragging the snippet, this is by how much they've dragged it.
    drag_shift: Option<TimeDiff>,
    path: BezPath,
    // It's expensive to always hit-test on the path.
    bbox: Rect,
    shape: SnippetShape,
    interior: SnippetInterior,
}

impl TimelineSnippet {
    fn snip(&self, data: &EditorState) -> Snip {
        match self.id {
            SnippetId::Draw(id) => Snip::Drawing(data.scribl.draw.snippet(id).clone()),
            SnippetId::Talk(id) => Snip::Audio(data.scribl.talk.snippet(id).clone()),
        }
    }

    fn fill_color(&self, data: &EditorState) -> Option<Color> {
        match self.id {
            SnippetId::Draw(_) => None,
            SnippetId::Talk(_) => {
                if data.selected_snippet == Some(self.id) {
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
        self.bbox.contains(p) && self.shape.rects.iter().any(|r| r.contains(p))
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
                if ev.mods.shift() {
                    self.drag_start = Some(x_pix(ev.pos.x));
                }
                ctx.request_paint();
                ctx.set_handled();
            }
            Event::MouseUp(ev) if ev.button.is_left() => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    if self.hot && self.contains(ev.pos) {
                        data.selected_snippet = Some(self.id);
                        ctx.set_handled();
                    }
                    if let Some(drag_shift) = self.drag_shift {
                        self.drag_start = None;
                        self.drag_shift = None;
                        data.shift_snippet(self.id, drag_shift);
                        ctx.request_paint();
                    }
                }
            }
            Event::MouseMove(ev) => {
                let new_hot = self.contains(ev.pos);
                if self.hot != new_hot {
                    self.hot = new_hot;
                    ctx.request_paint_rect(
                        self.bbox.inset(SNIPPET_SELECTED_STROKE_THICKNESS / 2.0),
                    );
                }
                if let Some(drag_start) = self.drag_start {
                    let old_drag_shift = self.drag_shift.unwrap_or(TimeDiff::from_micros(0));
                    self.drag_shift = Some(x_pix(ev.pos.x.max(0.0)) - drag_start);
                    ctx.request_paint();
                    let old_pos = pix_width(old_drag_shift);
                    let new_pos = pix_width(self.drag_shift.unwrap());
                    let bbox = self.bbox.inset(SNIPPET_STROKE_THICKNESS / 2.0);
                    ctx.request_paint_rect(bbox + Vec2::new(old_pos, 0.0));
                    ctx.request_paint_rect(bbox + Vec2::new(new_pos, 0.0));
                }
            }
            Event::KeyUp(ev) => {
                if ev.key == KbKey::Shift && self.drag_start.is_some() {
                    self.drag_start = None;
                    self.drag_shift = None;
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

    fn layout(&mut self, _: &mut LayoutCtx, bc: &BoxConstraints, _: &EditorState, _: &Env) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, _env: &Env) {
        let snippet = self.snip(data);
        let height = ctx.size().height;
        let is_selected = data.selected_snippet == Some(self.id);
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

            if is_selected || (self.hot && ctx.is_active()) {
                ctx.stroke(
                    &path,
                    &SNIPPET_SELECTED_STROKE_COLOR,
                    SNIPPET_SELECTED_STROKE_THICKNESS,
                );
            }
            if self.hot {
                ctx.stroke(&path, &SNIPPET_STROKE_COLOR, SNIPPET_STROKE_THICKNESS);
            }

            if let Some(drag_shift) = self.drag_shift {
                ctx.paint_with_z_index(1, move |ctx| {
                    ctx.with_save(|ctx| {
                        ctx.transform(Affine::translate((pix_width(drag_shift), 0.0)));
                        ctx.stroke(&path, &SNIPPET_STROKE_COLOR, SNIPPET_STROKE_THICKNESS);
                    });
                });
            }
        });
    }
}

impl TimelineInner {
    fn y_intervals<'a>(&'a self, x: f64) -> impl Iterator<Item = (SnippetId, f64, f64)> + 'a {
        self.children.iter().filter_map(move |(&id, snip)| {
            if let Some((y0, y1)) = snip.widget().y_interval(x) {
                Some((id, y0, y1))
            } else {
                None
            }
        })
    }

    fn selected<'a>(&'a self, data: &EditorState) -> Option<&'a TimelineSnippet> {
        data.selected_snippet
            .and_then(|id| self.children.get(&id).map(|w| w.widget()))
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
                    if time < self.visible_times.0 + CURSOR_BOUNDARY_PADDING
                        && self.visible_times.0 > Time::ZERO
                    {
                        let speed = (time - self.visible_times.0 - CURSOR_BOUNDARY_PADDING)
                            .as_micros() as f64
                            * factor;
                        self.cursor_drag_scroll_speed = Some(speed.max(-CURSOR_DRAG_SCROLL_SPEED));
                        ctx.request_anim_frame();
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
                    if id.is_some() {
                        data.selected_snippet = id;
                    }
                } else if c.is(cmd::SELECT_SNIPPET_BELOW) {
                    ctx.set_handled();

                    let y = y_int.map(|int| int.1).unwrap_or(0.0);
                    let id = self
                        .y_intervals(x)
                        .filter(|&(_id, y0, _y1)| y0 >= y)
                        .min_by(|a, b| a.2.partial_cmp(&b.2).unwrap())
                        .map(|a| a.0);
                    if id.is_some() {
                        data.selected_snippet = id;
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
        if !data.scribl.draw.same(&old_data.scribl.draw)
            || !data.scribl.talk.same(&old_data.scribl.talk)
        {
            ctx.request_layout();
            self.recreate_children(&data.scribl.draw, &data.scribl.talk);
            ctx.children_changed();
        } else {
            // Don't call update on the children if we just changed them -- we need to let
            // WidgetAdded be the first thing they see.
            for child in self.children.values_mut() {
                child.update(ctx, data, env);
            }
        }

        if old_data.mark != data.mark {
            ctx.request_paint();
        }
        if old_data.time() != data.time() {
            let invalid =
                TimelineInner::invalid_rect(old_data.time(), data.time(), ctx.size().height);
            ctx.request_paint_rect(invalid);
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
                self.recreate_children(&data.scribl.draw, &data.scribl.talk);
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
        let child_bc = BoxConstraints::new(Size::ZERO, size);
        for c in self.children.values_mut() {
            c.layout(ctx, &child_bc, data, env);
            c.set_origin(ctx, data, env, Point::ZERO);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        // Note that the width here may well be infinite. Intersecting with the
        // paint region will prevent us from trying to fill an infinite rect.
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size).intersect(ctx.region().bounding_box());
        let bg = env.get(druid::theme::BACKGROUND_DARK);
        ctx.fill(rect, &bg);

        for child in self.children.values_mut() {
            if ctx.region().intersects(child.widget().bbox) {
                child.paint(ctx, data, env);
            }
        }

        let cursor_x = pix_x(data.time());

        // Draw the mark.
        if let Some(mark_time) = data.mark {
            let mark_x = pix_x(mark_time);
            let rect = Rect::new(cursor_x, 0.0, mark_x, size.height);
            ctx.fill(rect, &SELECTION_FILL_COLOR);
            let mark_line = Line::new((mark_x, 0.0), (mark_x, size.height));
            ctx.stroke(mark_line, &Color::BLACK, CURSOR_THICKNESS);
            ctx.stroke_styled(
                mark_line,
                &Color::WHITE,
                1.0,
                &StrokeStyle::new().dash_pattern(&[2.0, 2.0]),
            );
        }

        let cursor_line = Line::new((cursor_x, 0.0), (cursor_x, size.height));
        // Draw a black "background" on the cursor for extra contrast.
        ctx.stroke(cursor_line, &Color::BLACK, CURSOR_THICKNESS);
        ctx.stroke(cursor_line, &Color::WHITE, 1.0);
    }
}
