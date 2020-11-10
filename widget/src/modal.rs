use std::sync::Arc;
use std::time::{Duration, Instant};

use druid::piet::FontFamily;
use druid::widget::prelude::*;
use druid::widget::LabelText;
use druid::{
    ArcStr, Color, Data, FontDescriptor, Point, Rect, Selector, SingleUse, TextLayout, TimerToken,
    Vec2, WidgetPod,
};

pub struct ModalHost<T, W> {
    mouse_pos: Point,
    inner: W,
    marker: std::marker::PhantomData<T>,
    cur_tooltip: Option<(Point, ArcStr)>,
    tooltip_layout: TextLayout<ArcStr>,
    modal: Option<WidgetPod<T, Box<dyn Widget<T>>>>,
}

pub struct TooltipHost<T, W> {
    text: LabelText<T>,
    timer: TimerToken,
    // If we are considering showing a tooltip, this will be the time of the last
    // mouse move event.
    last_mouse_move: Option<Instant>,
    inner: WidgetPod<T, W>,
}

pub trait TooltipExt<T: Data, W: Widget<T>> {
    fn tooltip<LT: Into<LabelText<T>>>(self, text: LT) -> TooltipHost<T, W>;
}

impl<T: Data, W: Widget<T> + 'static> TooltipExt<T, W> for W {
    fn tooltip<LT: Into<LabelText<T>>>(self, text: LT) -> TooltipHost<T, W> {
        TooltipHost {
            text: text.into(),
            timer: TimerToken::INVALID,
            last_mouse_move: None,
            inner: WidgetPod::new(self),
        }
    }
}

const TOOLTIP_DELAY: Duration = Duration::from_millis(500);
const TOOLTIP_DELAY_CHECK: Duration = Duration::from_millis(480);
const TOOLTIP_COLOR: Color = crate::UI_LIGHT_YELLOW;
const TOOLTIP_STROKE_WIDTH: f64 = 1.0;
const TOOLTIP_STROKE_COLOR: Color = Color::rgb8(0, 0, 0);
const TOOLTIP_TEXT_COLOR: Color = Color::rgb8(0, 0, 0);
// It looks better if we don't put the tooltip *right* on the tip of the mouse,
// because the mouse obstructs it.
const TOOLTIP_OFFSET: Vec2 = Vec2::new(5.0, 5.0);

const FONT_SIZE: f64 = 15.0;
const LINE_HEIGHT_FACTOR: f64 = 1.7;
const TEXT_TOP_GUESS_FACTOR: f64 = 0.15;
const X_PADDING: f64 = 6.0;

/// The argument is a string containing the tooltip text.
const SHOW_TOOLTIP: Selector<ArcStr> = Selector::new("scribl.show-tooltip");

impl<T: Data, W: Widget<T>> TooltipHost<T, W> {
    pub fn child(&self) -> &W {
        self.inner.widget()
    }

    pub fn child_mut(&mut self) -> &mut W {
        self.inner.widget_mut()
    }
}

impl<T: Data, W: Widget<T>> Widget<T> for TooltipHost<T, W> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        match ev {
            Event::MouseDown(_) | Event::MouseUp(_) => {
                self.timer = TimerToken::INVALID;
                self.last_mouse_move = None;
            }
            Event::MouseMove(_) => {
                self.last_mouse_move = if ctx.is_hot() {
                    if self.timer == TimerToken::INVALID {
                        self.timer = ctx.request_timer(TOOLTIP_DELAY);
                    }
                    Some(Instant::now())
                } else {
                    None
                };
            }
            Event::Timer(tok) if tok == &self.timer => {
                self.timer = TimerToken::INVALID;
                if let Some(move_time) = self.last_mouse_move {
                    let elapsed = Instant::now().duration_since(move_time);
                    if elapsed > TOOLTIP_DELAY_CHECK {
                        self.text.resolve(data, env);
                        ctx.submit_command(SHOW_TOOLTIP.with(self.text.display_text()));
                        self.timer = TimerToken::INVALID;
                        self.last_mouse_move = None;
                    } else {
                        self.timer = ctx.request_timer(TOOLTIP_DELAY - elapsed);
                    }
                }
            }
            _ => {}
        }
        self.inner.event(ctx, ev, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        self.inner.lifecycle(ctx, ev, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        self.inner.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let size = self.inner.layout(ctx, bc, data, env);
        self.inner.set_layout_rect(ctx, data, env, size.to_rect());
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.inner.paint(ctx, data, env);
    }
}

impl ModalHost<(), ()> {
    pub const DISMISS_MODAL: Selector<()> = Selector::new("scribl.dismiss-modal");
}

impl<T> ModalHost<T, ()> {
    pub const SHOW_MODAL: Selector<SingleUse<Box<dyn Widget<T>>>> =
        Selector::new("scribl.show-modal");
}

impl<T, W: Widget<T>> ModalHost<T, W> {
    pub fn new(inner: W) -> ModalHost<T, W> {
        ModalHost {
            mouse_pos: Point::ZERO,
            inner,
            marker: std::marker::PhantomData,
            cur_tooltip: None,
            tooltip_layout: TextLayout::new(),
            modal: None,
        }
    }
}

impl<T: Data, W: Widget<T>> Widget<T> for ModalHost<T, W> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        match ev {
            Event::MouseMove(ev) => {
                self.mouse_pos = ev.pos;
            }
            Event::Command(c) => {
                if let Some(string) = c.get(SHOW_TOOLTIP) {
                    self.cur_tooltip = Some((self.mouse_pos, Arc::clone(string)));
                    ctx.request_paint();
                    ctx.set_handled();
                } else if let Some(modal) = c.get(ModalHost::SHOW_MODAL) {
                    if self.modal.is_none() {
                        self.modal = Some(WidgetPod::new(modal.take().unwrap()));
                        ctx.children_changed();
                    } else {
                        log::warn!("already showing modal");
                    }
                    ctx.set_handled();
                } else if c.is(ModalHost::DISMISS_MODAL) {
                    if self.modal.is_some() {
                        self.modal = None;
                        ctx.children_changed();
                    } else {
                        log::warn!("not showing modal");
                    }
                    ctx.set_handled();
                }
            }
            _ => {}
        }

        if is_user_input(ev) {
            if self.cur_tooltip.is_some() {
                self.cur_tooltip = None;
                ctx.request_paint();
            }

            match self.modal.as_mut() {
                Some(modal) => modal.event(ctx, ev, data, env),
                None => self.inner.event(ctx, ev, data, env),
            }
        } else {
            self.inner.event(ctx, ev, data, env)
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        if let Some(ref mut modal) = self.modal {
            modal.lifecycle(ctx, ev, data, env);
        }
        self.inner.lifecycle(ctx, ev, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        if let Some(ref mut modal) = self.modal {
            modal.update(ctx, data, env);
        }
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let size = self.inner.layout(ctx, bc, data, env);
        if let Some(modal) = self.modal.as_mut() {
            let modal_constraints = BoxConstraints::new(Size::ZERO, size);
            let modal_size = modal.layout(ctx, &modal_constraints, data, env);
            let modal_origin = (size.to_vec2() - modal_size.to_vec2()) / 2.0;
            let modal_frame = Rect::from_origin_size(modal_origin.to_point(), modal_size);
            modal.set_layout_rect(ctx, data, env, modal_frame);
        }
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.inner.paint(ctx, data, env);

        if let Some(modal) = self.modal.as_mut() {
            let frame = ctx.size().to_rect();
            ctx.fill(frame, &Color::BLACK.with_alpha(0.45));
            let modal_rect = modal.layout_rect() + Vec2::new(5.0, 5.0);
            let blur_color = Color::grey8(100);
            ctx.blurred_rect(modal_rect, 5.0, &blur_color);
            modal.paint(ctx, data, env);
        }

        if let Some((point, string)) = &self.cur_tooltip {
            let mut tooltip_origin = *point + TOOLTIP_OFFSET;
            self.tooltip_layout
                .set_font(FontDescriptor::new(FontFamily::SANS_SERIF).with_size(FONT_SIZE));
            self.tooltip_layout.set_text(Arc::clone(string));
            self.tooltip_layout.set_text_color(TOOLTIP_TEXT_COLOR);
            self.tooltip_layout.rebuild_if_needed(&mut ctx.text(), env);
            let line_height = FONT_SIZE * LINE_HEIGHT_FACTOR;
            let size = ctx.size();
            let text_size = self.tooltip_layout.size();

            // If necessary, try to offset the tooltip so that it fits in the widget.
            if tooltip_origin.y + line_height > size.height {
                tooltip_origin.y = size.height - line_height;
            }
            if tooltip_origin.x + text_size.width > size.width {
                tooltip_origin.x = size.width - text_size.width;
            }

            let rect = Rect::from_origin_size(
                tooltip_origin,
                (text_size.width + X_PADDING * 2.0, line_height),
            )
            .inset(-TOOLTIP_STROKE_WIDTH / 2.0)
            .to_rounded_rect(env.get(druid::theme::BUTTON_BORDER_RADIUS));
            let text_origin =
                tooltip_origin + Vec2::new(X_PADDING, line_height * TEXT_TOP_GUESS_FACTOR);

            ctx.fill(rect, &TOOLTIP_COLOR);
            ctx.stroke(rect, &TOOLTIP_STROKE_COLOR, TOOLTIP_STROKE_WIDTH);
            self.tooltip_layout.draw(ctx, text_origin);
        }
    }
}

fn is_user_input(event: &Event) -> bool {
    match event {
        Event::MouseUp(_)
        | Event::MouseDown(_)
        | Event::MouseMove(_)
        | Event::KeyUp(_)
        | Event::KeyDown(_)
        | Event::Paste(_)
        | Event::Wheel(_)
        | Event::Zoom(_) => true,
        _ => false,
    }
}
