use druid::kurbo::{BezPath, Line, Vec2};
use druid::theme;
use druid::widget::{Controller, Scroll};
use druid::{
    Affine, BoxConstraints, Color, Command, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle,
    LifeCycleCtx, PaintCtx, Point, Rect, RenderContext, Size, UpdateCtx, Widget, WidgetExt,
    WidgetPod,
};
use std::collections::HashMap;

use scribl_curves::{time, SnippetData, SnippetId, SnippetsData, Time, TimeDiff};

use crate::audio::{AudioSnippetData, AudioSnippetId, AudioSnippetsData};
use crate::cmd;
use crate::editor_state::EditorState;
use crate::snippet_layout;

const SNIPPET_HEIGHT: f64 = 20.0;
const MIN_NUM_ROWS: usize = 5;
const PIXELS_PER_USEC: f64 = 100.0 / 1000000.0;
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
    // The shape of the waveform. This is rendered with respect to a height
    // going from -1 to 1.
    wave: BezPath,
}

/// The cached "waveform" of a drawing snippet.
struct DrawingWaveform {
    segments: Vec<(Time, Time, Color)>,
}

/// The data of a snippet (either a drawing snippet or an audio snippet).
#[derive(Clone, Data)]
enum Snip {
    Drawing(SnippetData),
    Audio(AudioSnippetData),
}

impl AudioWaveform {
    fn from_audio(data: AudioSnippetData) -> AudioWaveform {
        // Converts a PCM sample to a y coordinate. This could use some more
        // thought and/or testing. Like, should we be taking a logarithm somewhere?
        let audio_height = |x: f64| -> f64 {
            (x * (data.multiplier() as f64) / std::i16::MAX as f64)
                .max(-1.0)
                .min(1.0)
        };

        let width = pix_width(data.end_time() - data.start_time());
        let pix_per_sample = 5;
        let buf = data.buf();
        let mut mags = Vec::with_capacity((width as usize) / pix_per_sample);
        let mut path = BezPath::new();
        path.move_to((0.0, 0.0));
        for p in (0..(width as usize)).step_by(pix_per_sample) {
            let start_time = x_pix(p as f64) - time::ZERO;
            let end_time = x_pix((p + pix_per_sample) as f64) - time::ZERO;
            let start_idx =
                (start_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize).min(buf.len());
            let end_idx =
                (end_time.as_audio_idx(crate::audio::SAMPLE_RATE) as usize).min(buf.len());
            let sub_buf = &buf[start_idx..end_idx];

            let mag = (sub_buf.iter().cloned().max().unwrap_or(0) as f64
                - sub_buf.iter().cloned().min().unwrap_or(0) as f64)
                / 2.0;
            path.line_to((p as f64, audio_height(mag)));
            mags.push((p, mag));
        }

        for (p, mag) in mags.into_iter().rev() {
            path.line_to((p as f64, -audio_height(mag)));
        }
        path.close_path();
        AudioWaveform { wave: path }
    }
}

impl DrawingWaveform {
    fn from_snippet(data: &SnippetData) -> DrawingWaveform {
        let mut segs = Vec::new();
        for seg in data.curve.segments() {
            if seg.times.is_empty() {
                continue;
            }
            let first_time = *seg.times.first().unwrap();
            let last_time = *seg.times.last().unwrap();
            segs.push((
                data.lerp.lerp_clamped(first_time),
                data.lerp.lerp_clamped(last_time),
                seg.style.color,
            ));
        }
        DrawingWaveform { segments: segs }
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
}

/// The main timeline widget.
struct TimelineInner {
    // The timeline is organized in rows, and this map associates each id to a
    // row (with 0 being the topmost row).
    snippet_offsets: HashMap<Id, usize>,
    num_rows: usize,
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

            child.scroll(Vec2 { x: delta_x, y: 0.0 }, size);
        }
        child.update(ctx, old_data, data, env);
    }
}

impl Default for TimelineInner {
    fn default() -> TimelineInner {
        TimelineInner {
            snippet_offsets: HashMap::new(),
            num_rows: MIN_NUM_ROWS,
            children: HashMap::new(),
        }
    }
}

impl TimelineInner {
    // Recreates the child widgets, and organizes them into rows so that they don't overlap.
    fn recreate_children(&mut self, snippets: &SnippetsData, audio: &AudioSnippetsData) {
        let draw_offsets = snippet_layout::layout(snippets.snippets());
        let audio_offsets = snippet_layout::layout(audio.snippets());
        self.num_rows = (draw_offsets.num_rows + audio_offsets.num_rows).max(MIN_NUM_ROWS);

        self.snippet_offsets.clear();
        self.children.clear();
        for (&id, &offset) in &draw_offsets.positions {
            let snip = snippets.snippet(id);
            let id = Id::Drawing(id);
            self.snippet_offsets.insert(id, offset);
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    wave: None,
                    segs: Some(DrawingWaveform::from_snippet(&snip)),
                }),
            );
        }
        for (&id, &offset) in &audio_offsets.positions {
            let audio_data = audio.snippet(id);
            let id = Id::Audio(id);
            self.snippet_offsets.insert(id, self.num_rows - offset - 1);
            self.children.insert(
                id,
                WidgetPod::new(TimelineSnippet {
                    id,
                    wave: Some(AudioWaveform::from_audio(audio_data.clone())),
                    segs: None,
                }),
            );
        }
    }
}

/// A widget representing a single snippet (audio or drawing) in the timeline.
struct TimelineSnippet {
    // The id of the snippet that this widget represents.
    id: Id,
    // If the snippet is an audio snippet, a precalculated waveform.
    wave: Option<AudioWaveform>,
    // If the segment is a drawing snippet, a precalculated waveform.
    segs: Option<DrawingWaveform>,
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

    /// Draws the "interior" of the snippet (i.e., everything but the bounding rect).
    fn render_interior(&self, ctx: &mut PaintCtx, snip: &Snip, _width: f64, height: f64) {
        match snip {
            Snip::Audio(_data) => {
                ctx.with_save(|ctx| {
                    // The precomputed waveform is based on a vertical scale of
                    // [-1, 1], so transform it to [0, height]
                    ctx.transform(
                        Affine::translate((0.0, height / 2.0))
                            * Affine::scale_non_uniform(1.0, height / 2.0),
                    );
                    let wave = self
                        .wave
                        .as_ref()
                        .expect("audio snippet should have a cached waveform");
                    ctx.fill(&wave.wave, &SNIPPET_WAVEFORM_COLOR);
                });
            }
            Snip::Drawing(data) => {
                let segs = self
                    .segs
                    .as_ref()
                    .expect("drawing widget should have cached segment extents");
                for &(start, end, ref color) in &segs.segments {
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
            Event::MouseDown(ev) if ev.button.is_left() => {
                ctx.set_active(true);
                ctx.set_handled();
            }
            Event::MouseUp(ev) if ev.button.is_left() => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    if ctx.is_hot() {
                        match self.id {
                            Id::Drawing(id) => data.selected_snippet = id.into(),
                            Id::Audio(id) => data.selected_snippet = id.into(),
                        }
                        ctx.request_paint();
                        ctx.set_handled();
                        ctx.set_menu(crate::menus::make_menu(data));
                    }
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
            if let Snip::Audio(data) = snip {
                self.wave = Some(AudioWaveform::from_audio(data));
            }
            ctx.request_paint();
        }

        if old_data.selected_snippet != data.selected_snippet {
            ctx.request_paint();
        }
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        _data: &EditorState,
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
        data: &EditorState,
        _env: &Env,
    ) -> Size {
        let width = self.width(data);
        let height = SNIPPET_HEIGHT;
        bc.constrain((width, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        let snippet = self.snip(data);
        let width = self.width(data);
        let height = SNIPPET_HEIGHT;
        let radius = env.get(theme::BUTTON_BORDER_RADIUS);
        let is_selected = data.selected_snippet == self.id.to_maybe_id();

        // Logically, untruncated snippets have infinite width. But druid
        // doesn't support drawing rectangles of infinite width, so we truncate
        // our rectangle to be just a little bit bigger than the drawing region.
        let bounding_rect = ctx
            .region()
            .to_rect()
            .inflate(radius + 1.0, std::f64::INFINITY);
        let stroke_thick = if ctx.is_hot() && !is_selected {
            SNIPPET_STROKE_THICKNESS
        } else {
            0.0
        };
        let rect = Rect::from_origin_size(Point::ZERO, (width, height))
            .inset(-SNIPPET_STROKE_THICKNESS)
            .intersect(bounding_rect)
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let inner_rect = Rect::from_origin_size(Point::ZERO, (width, height))
            .inset(-SNIPPET_STROKE_THICKNESS - stroke_thick / 2.0)
            .intersect(bounding_rect)
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let stroke_color = self.stroke_color();
        let fill_color = self.fill_color(data);

        ctx.with_save(|ctx| {
            let clip = ctx.region().to_rect();
            ctx.clip(clip);
            ctx.fill(&rect, &fill_color);
            if stroke_thick > 0.0 {
                ctx.stroke(&inner_rect, &stroke_color, stroke_thick);
            }
            ctx.clip(&inner_rect);
            self.render_interior(ctx, &snippet, width, height);
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
                ctx.request_paint();
            }
            Event::MouseMove(ev) => {
                // On click-and-drag, we change the time with the drag.
                if ctx.is_active() {
                    let time = Time::from_micros((ev.pos.x.max(0.0) / PIXELS_PER_USEC) as i64);
                    ctx.submit_command(Command::new(cmd::WARP_TIME_TO, time), None);
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
        if old_data.time() != data.time() || old_data.mark != data.mark {
            ctx.request_paint();
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
        for (&id, &offset) in &self.snippet_offsets {
            let child = self.children.get_mut(&id).unwrap();
            let x = pix_x(child.widget().snip(data).start_time());
            let y = offset as f64 * SNIPPET_HEIGHT;

            let size = child.layout(ctx, bc, data, env);
            child.set_layout_rect(ctx, data, env, Rect::from_origin_size((x, y), size));
        }

        let height = SNIPPET_HEIGHT * self.num_rows as f64;
        bc.constrain((std::f64::INFINITY, height))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        // Note that the width here may well be infinite. Intersecting with the
        // paint region will prevent us from trying to fill an infinite rect.
        let size = ctx.size();
        let rect = Rect::from_origin_size(Point::ZERO, size).intersect(ctx.region().to_rect());
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
