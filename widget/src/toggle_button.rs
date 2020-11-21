use druid::kurbo::Vec2;
use druid::widget::prelude::*;
use druid::widget::Painter;
use druid::{theme, Data, Insets, RenderContext, Size, WidgetPod};
use std::rc::Rc;

use crate::{Icon, IconWidget};

// When drawing toggle buttons really close to each other (say in a radio group), we want to draw
// the lowered buttons first, and then the shadows and raised buttons after.  This enum is a little
// hack to enable drawing just one of those two layers.
#[derive(PartialEq)]
pub(crate) enum Layer {
    Top,
    Shadow,
    Bottom,
    All,
}

pub struct ToggleButton<T> {
    inner: WidgetPod<T, IconWidget<T>>,
    layer: Layer,
    toggle_state: Rc<dyn Fn(&T) -> bool + 'static>,
    toggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
    untoggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
}

impl<T: Data> ToggleButton<T> {
    pub fn from_icon(
        icon: &Icon,
        toggle_state: impl Fn(&T) -> bool + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        let toggle_state = Rc::new(toggle_state);
        let toggle_state_clone = Rc::clone(&toggle_state);

        let icon_painter = move |ctx: &mut PaintCtx, data: &T, env: &Env| {
            let color = if toggle_state_clone(data) {
                env.get(crate::BUTTON_ICON_SELECTED_COLOR)
            } else {
                env.get(crate::BUTTON_ICON_COLOR)
            };
            let rect = ctx.size().to_rect();
            ctx.fill(rect, &color);
        };

        ToggleButton {
            inner: WidgetPod::new(icon.to_widget(Painter::new(icon_painter))),
            layer: Layer::All,
            toggle_state,
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
    }

    pub fn from_icon_widget(
        icon_widget: IconWidget<T>,
        toggle_state: impl Fn(&T) -> bool + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        ToggleButton {
            inner: WidgetPod::new(icon_widget),
            layer: Layer::All,
            toggle_state: Rc::new(toggle_state),
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
    }

    pub fn icon_width(mut self, width: f64) -> Self {
        self.inner.widget_mut().set_width(width);
        self
    }

    pub fn icon_height(mut self, height: f64) -> Self {
        self.inner.widget_mut().set_height(height);
        self
    }

    pub(crate) fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }
}

impl<T: Data> Widget<T> for ToggleButton<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        match event {
            Event::MouseDown(_) => {
                ctx.set_active(true);
                ctx.request_paint();
                ctx.set_handled();
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    ctx.request_paint();
                    let state = (self.toggle_state)(data);
                    if ctx.is_hot() {
                        if state {
                            (self.untoggle_action)(ctx, data, env)
                        } else {
                            (self.toggle_action)(ctx, data, env)
                        }
                    }
                }
                ctx.set_handled();
            }
            _ => {}
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, _env: &Env) {
        if (self.toggle_state)(old_data) != (self.toggle_state)(data) {
            ctx.request_paint();
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let shadow_radius = env.get(crate::DROP_SHADOW_RADIUS);
        let child_bc = bc.shrink((2.0 * padding, 2.0 * padding));
        let child_size = self.inner.layout(ctx, &child_bc, data, env);
        self.inner
            .set_origin(ctx, data, env, (padding, padding).into());
        let size = child_size + Size::new(2.0 * padding, 2.0 * padding);
        ctx.set_paint_insets(Insets::uniform(shadow_radius));

        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let shadow_radius = env.get(crate::DROP_SHADOW_RADIUS);
        let shadow_color = env.get(crate::DROP_SHADOW_COLOR);
        let button_color = env.get(crate::BUTTON_ICON_BUTTON_COLOR);
        let stroke_color = env.get(crate::BUTTON_ICON_HOT_STROKE_COLOR);
        let stroke_thickness = env.get(crate::BUTTON_ICON_HOT_STROKE_THICKNESS);

        let is_toggled = (self.toggle_state)(data);
        let is_hot = ctx.is_hot();
        let is_pressed = is_toggled || (ctx.is_active() && is_hot);

        let shadow_rect = ctx.size().to_rect();
        let button_rect = shadow_rect.to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let draw_bottom = self.layer == Layer::All || self.layer == Layer::Bottom;
        let draw_top = self.layer == Layer::All || self.layer == Layer::Top;
        let draw_shadow = self.layer == Layer::All || self.layer == Layer::Shadow;

        if is_pressed && draw_bottom {
            ctx.fill(button_rect, &button_color);
            self.inner.paint(ctx, data, env);
        }
        if draw_shadow {
            if is_pressed {
                ctx.with_save(|ctx| {
                    let rect = (ctx.size() + Size::new(100.0, 100.0)).to_rect();
                    let up = Vec2::new(0.0, -rect.height());
                    let down = Vec2::new(0.0, ctx.size().height);
                    let left = Vec2::new(-rect.width(), 0.0);
                    let right = Vec2::new(ctx.size().width, 0.0);
                    // The inner shadows tend to look "stronger", so make them smaller.
                    let r = shadow_radius / 2.0;

                    ctx.clip(button_rect);
                    ctx.blurred_rect(rect + up, r, &shadow_color);
                    ctx.blurred_rect(rect + down, r, &shadow_color);
                    ctx.blurred_rect(rect + left, r, &shadow_color);
                    ctx.blurred_rect(rect + right, r, &shadow_color);
                });
            } else {
                ctx.blurred_rect(shadow_rect, shadow_radius, &shadow_color);
            }
        }
        if !is_pressed && draw_top {
            ctx.fill(button_rect, &button_color);
            self.inner.paint(ctx, data, env);
            if is_hot {
                let rect = shadow_rect
                    .inset(-stroke_thickness / 2.0)
                    .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
                ctx.stroke(rect, &stroke_color, stroke_thickness);
            }
        }
    }
}
