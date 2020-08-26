use std::time::{Duration, Instant};

use crate::EditorState;

use druid::piet::{FontFamily, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::widget::{Controller, ControllerHost, LabelText};
use druid::{
    Color, Command, Data, Point, Rect, Selector, SingleUse, TimerToken, Vec2, WidgetExt, WidgetPod,
};

pub struct ModalHost<T, W> {
    mouse_pos: Point,
    inner: W,
    marker: std::marker::PhantomData<T>,
    cur_tooltip: Option<(Point, String)>,
    modal: Option<WidgetPod<T, Box<dyn Widget<T>>>>,
}

pub struct TooltipGuest<T> {
    text: LabelText<T>,
    timer: TimerToken,
    // If we are considering showing a tooltip, this will be the time of the last
    // mouse move event.
    last_mouse_move: Option<Instant>,
}

pub trait TooltipExt<T: Data, W: Widget<T>> {
    fn tooltip<LT: Into<LabelText<T>>>(self, text: LT) -> ControllerHost<W, TooltipGuest<T>>;
}

impl<T: Data, W: Widget<T> + 'static> TooltipExt<T, W> for W {
    fn tooltip<LT: Into<LabelText<T>>>(self, text: LT) -> ControllerHost<W, TooltipGuest<T>> {
        self.controller(TooltipGuest {
            text: text.into(),
            timer: TimerToken::INVALID,
            last_mouse_move: None,
        })
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
const SHOW_TOOLTIP: Selector<String> = Selector::new("scribl.show-tooltip");

impl<T: Data, W: Widget<T>> Controller<T, W> for TooltipGuest<T> {
    fn event(&mut self, child: &mut W, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
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
                        ctx.submit_command(
                            Command::new(SHOW_TOOLTIP, self.text.display_text().to_owned()),
                            None,
                        );
                        self.timer = TimerToken::INVALID;
                        self.last_mouse_move = None;
                    } else {
                        self.timer = ctx.request_timer(TOOLTIP_DELAY - elapsed);
                    }
                }
            }
            _ => {}
        }
        child.event(ctx, ev, data, env);
    }
}

impl ModalHost<(), ()> {
    pub const DISMISS_MODAL: Selector<()> = Selector::new("scribl.dismiss-modal");
}

impl<T> ModalHost<T, ()> {
    pub const SHOW_MODAL: Selector<SingleUse<Box<dyn Widget<T>>>> =
        Selector::new("scribl.show-modal");
}

impl<W: Widget<EditorState>> ModalHost<EditorState, W> {
    pub fn new(inner: W) -> ModalHost<EditorState, W> {
        ModalHost {
            mouse_pos: Point::ZERO,
            inner,
            marker: std::marker::PhantomData,
            cur_tooltip: None,
            modal: None,
        }
    }
}

// We specialize to EditorState because it lets us set the menus.
impl<W: Widget<EditorState>> Widget<EditorState> for ModalHost<EditorState, W> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut EditorState, env: &Env) {
        match ev {
            Event::MouseMove(ev) => {
                self.mouse_pos = ev.pos;
            }
            Event::Command(c) => {
                if let Some(string) = c.get(SHOW_TOOLTIP) {
                    self.cur_tooltip = Some((self.mouse_pos, string.clone()));
                    ctx.request_paint();
                    ctx.set_handled();
                } else if let Some(modal) = c.get(ModalHost::SHOW_MODAL) {
                    if self.modal.is_none() {
                        self.modal = Some(WidgetPod::new(modal.take().unwrap()));
                        ctx.children_changed();
                        ctx.set_menu(druid::MenuDesc::<crate::AppState>::empty());
                    } else {
                        log::warn!("already showing modal");
                    }
                    ctx.set_handled();
                } else if c.is(ModalHost::DISMISS_MODAL) {
                    if self.modal.is_some() {
                        self.modal = None;
                        ctx.children_changed();
                        ctx.set_menu(crate::menus::make_menu(data));
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

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &EditorState, env: &Env) {
        if let Some(ref mut modal) = self.modal {
            modal.lifecycle(ctx, ev, data, env);
        }
        self.inner.lifecycle(ctx, ev, data, env)
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        env: &Env,
    ) {
        if let Some(ref mut modal) = self.modal {
            modal.update(ctx, data, env);
        }
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        env: &Env,
    ) -> Size {
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

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
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
            let mut text = ctx.text();
            let font = text
                .font_family(env.get(druid::theme::FONT_NAME))
                .unwrap_or(FontFamily::SANS_SERIF);
            let layout = text
                .new_text_layout(string)
                .font(font, FONT_SIZE)
                .text_color(TOOLTIP_TEXT_COLOR)
                .build()
                .unwrap();
            let line_height = FONT_SIZE * LINE_HEIGHT_FACTOR;
            let size = ctx.size();

            // If necessary, try to offset the tooltip so that it fits in the widget.
            if tooltip_origin.y + line_height > size.height {
                tooltip_origin.y = size.height - line_height;
            }
            if tooltip_origin.x + layout.size().width > size.width {
                tooltip_origin.x = size.width - layout.size().width;
            }

            let rect = Rect::from_origin_size(
                tooltip_origin,
                (layout.size().width + X_PADDING * 2.0, line_height),
            )
            .inset(-TOOLTIP_STROKE_WIDTH / 2.0)
            .to_rounded_rect(env.get(druid::theme::BUTTON_BORDER_RADIUS));
            let text_origin =
                tooltip_origin + Vec2::new(X_PADDING, line_height * TEXT_TOP_GUESS_FACTOR);

            ctx.fill(rect, &TOOLTIP_COLOR);
            ctx.stroke(rect, &TOOLTIP_STROKE_COLOR, TOOLTIP_STROKE_WIDTH);
            ctx.draw_text(&layout, text_origin);
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
