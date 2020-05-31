use std::time::{Duration, Instant};

use druid::piet::{FontBuilder, Text, TextLayout, TextLayoutBuilder};
use druid::widget::prelude::*;
use druid::widget::{Controller, ControllerHost, LabelText};
use druid::{Color, Command, Data, Point, Rect, Selector, TimerToken, Vec2, WidgetExt};

pub struct TooltipHost<T, W: Widget<T>> {
    mouse_pos: Point,
    inner: W,
    marker: std::marker::PhantomData<T>,
    cur_tooltip: Option<(Point, String)>,
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
const BASELINE_GUESS_FACTOR: f64 = 0.7;
const X_PADDING: f64 = 6.0;

/// The argument is a string containing the tooltip text.
const SHOW_TOOLTIP: Selector<String> = Selector::new("scribble.show-tooltip");

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

impl<T, W: Widget<T>> TooltipHost<T, W> {
    pub fn new(inner: W) -> TooltipHost<T, W> {
        TooltipHost {
            mouse_pos: Point::ZERO,
            inner,
            marker: std::marker::PhantomData,
            cur_tooltip: None,
        }
    }
}

impl<T, W: Widget<T>> Widget<T> for TooltipHost<T, W> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        match ev {
            Event::MouseMove(ev) => {
                self.mouse_pos = ev.pos;
                if self.cur_tooltip.is_some() {
                    self.cur_tooltip = None;
                    // TODO: only paint the tooltip rect
                    ctx.request_paint();
                }
            }
            Event::MouseUp(_)
            | Event::MouseDown(_)
            | Event::KeyUp(_)
            | Event::KeyDown(_)
            | Event::Wheel(_) => {
                if self.cur_tooltip.is_some() {
                    self.cur_tooltip = None;
                    ctx.request_paint();
                }
            }
            Event::Command(c) => {
                if let Some(string) = c.get(SHOW_TOOLTIP) {
                    self.cur_tooltip = Some((self.mouse_pos, string.clone()));
                    ctx.request_paint();
                }
            }
            _ => {}
        }
        self.inner.event(ctx, ev, data, env)
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        self.inner.lifecycle(ctx, ev, data, env)
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        self.inner.update(ctx, old_data, data, env)
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        self.inner.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        self.inner.paint(ctx, data, env);

        if let Some((point, string)) = &self.cur_tooltip {
            let mut tooltip_origin = *point + TOOLTIP_OFFSET;
            let font = ctx
                .text()
                .new_font_by_name(env.get(druid::theme::FONT_NAME), FONT_SIZE)
                .build()
                .unwrap();
            let layout = ctx
                .text()
                .new_text_layout(&font, string, None)
                .build()
                .unwrap();
            let line_height = FONT_SIZE * LINE_HEIGHT_FACTOR;
            let size = ctx.size();

            // If necessary, try to offset the tooltip so that it fits in the widget.
            if tooltip_origin.y + line_height > size.height {
                tooltip_origin.y = size.height - line_height;
            }
            if tooltip_origin.x + layout.width() > size.width {
                tooltip_origin.x = size.width - layout.width();
            }

            let rect = Rect::from_origin_size(
                tooltip_origin,
                (layout.width() + X_PADDING * 2.0, line_height),
            )
            .inset(-TOOLTIP_STROKE_WIDTH / 2.0)
            .to_rounded_rect(env.get(druid::theme::BUTTON_BORDER_RADIUS));
            let text_origin =
                tooltip_origin + Vec2::new(X_PADDING, line_height * BASELINE_GUESS_FACTOR);

            ctx.fill(rect, &TOOLTIP_COLOR);
            ctx.stroke(rect, &TOOLTIP_STROKE_COLOR, TOOLTIP_STROKE_WIDTH);
            ctx.draw_text(&layout, text_origin, &TOOLTIP_TEXT_COLOR);
        }
    }
}
