use druid::kurbo::Vec2;
use druid::widget::prelude::*;
use druid::widget::LabelText;
use druid::{theme, Data, Insets, Point, RenderContext, Size, WidgetExt, WidgetPod};

use crate::{Icon, Shadow, TooltipExt};

/// A [`ToggleButton`] that doesn't draw its drop shadow. This is potentially useful for combining
/// toggle buttons in a way that the shadows need to be handled simultaneously. For example, this
/// is used in [`RadioGroup`](crate::RadioGroup).
pub struct ShadowlessToggleButton<T> {
    inner: WidgetPod<T, Box<dyn Widget<T>>>,
    down: bool,
    // We often combine this widget with a drop shadow, in which case its paint insets need to
    // include the shadow insets.
    insets: Insets,
    toggle_state: Box<dyn Fn(&T) -> bool + 'static>,
    toggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
    untoggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
}

pub struct ToggleButton<T> {
    button: WidgetPod<T, ShadowlessToggleButton<T>>,
    shadow: WidgetPod<T, Shadow>,
}

impl<T: Data> ShadowlessToggleButton<T> {
    pub fn from_icon(
        icon: &Icon,
        padding: f64,
        tooltip: impl Into<LabelText<T>>,
        toggle_state: impl Fn(&T) -> bool + Clone + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ShadowlessToggleButton<T> {
        let lens = crate::lens::read_map(toggle_state.clone());
        let inner = icon
            .to_widget()
            .lens(lens)
            .padding(padding)
            .tooltip(tooltip);

        ShadowlessToggleButton {
            inner: WidgetPod::new(Box::new(inner)),
            down: false,
            insets: Insets::ZERO,
            toggle_state: Box::new(toggle_state),
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
    }

    pub fn from_widget(
        widget: impl Widget<T> + 'static,
        toggle_state: impl Fn(&T) -> bool + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ShadowlessToggleButton<T> {
        ShadowlessToggleButton {
            inner: WidgetPod::new(Box::new(widget)),
            down: false,
            insets: Insets::ZERO,
            toggle_state: Box::new(toggle_state),
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
    }

    pub fn is_down(&self) -> bool {
        self.down
    }

    pub fn set_insets(&mut self, insets: Insets) {
        self.insets = insets;
    }
}

impl<T: Data> ToggleButton<T> {
    pub fn from_icon(
        icon: &Icon,
        padding: f64,
        tooltip: impl Into<LabelText<T>>,
        toggle_state: impl Fn(&T) -> bool + Clone + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        let button = ShadowlessToggleButton::from_icon(
            icon,
            padding,
            tooltip,
            toggle_state,
            toggle_action,
            untoggle_action,
        );
        ToggleButton {
            button: WidgetPod::new(button),
            shadow: WidgetPod::new(Shadow),
        }
    }

    pub fn from_widget(
        icon_widget: impl Widget<T> + 'static,
        toggle_state: impl Fn(&T) -> bool + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        let button = ShadowlessToggleButton::from_widget(
            icon_widget,
            toggle_state,
            toggle_action,
            untoggle_action,
        );
        ToggleButton {
            button: WidgetPod::new(button),
            shadow: WidgetPod::new(Shadow),
        }
    }
}

impl<T: Data> Widget<T> for ShadowlessToggleButton<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        match event {
            Event::MouseDown(_) => {
                self.down = true;
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
                    self.down = (self.toggle_state)(data);
                }
                ctx.set_handled();
            }
            _ => {}
        }
        self.inner.event(ctx, event, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        match event {
            LifeCycle::HotChanged(_) => {
                self.down = (self.toggle_state)(data) || (ctx.is_active() && ctx.is_hot());
                ctx.request_paint();
            }
            LifeCycle::WidgetAdded => {
                self.down = (self.toggle_state)(data) || (ctx.is_active() && ctx.is_hot());
            }
            _ => {}
        }
        self.inner.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        self.down = (self.toggle_state)(data) || (ctx.is_active() && ctx.is_hot());
        if (self.toggle_state)(old_data) != (self.toggle_state)(data) {
            ctx.request_paint();
        }
        self.inner.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let size = self.inner.layout(ctx, &bc, data, env);
        self.inner.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets(self.insets);

        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let shadow_color = env.get(crate::DROP_SHADOW_COLOR);
        let shadow_radius = env.get(crate::DROP_SHADOW_RADIUS);
        let button_color = env.get(crate::BUTTON_ICON_BUTTON_COLOR);
        let stroke_color = env.get(crate::BUTTON_ICON_HOT_STROKE_COLOR);
        let stroke_thickness = env.get(crate::BUTTON_ICON_HOT_STROKE_THICKNESS);

        let button_rect = ctx
            .size()
            .to_rect()
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        ctx.fill(button_rect, &button_color);
        self.inner.paint(ctx, data, env);
        if ctx.is_hot() {
            let rect = ctx
                .size()
                .to_rect()
                .inset(-stroke_thickness / 2.0)
                .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
            ctx.stroke(rect, &stroke_color, stroke_thickness);
        }

        if self.is_down() {
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
        }
    }
}

impl<T: Data> Widget<T> for ToggleButton<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        self.button.event(ctx, event, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        self.button.lifecycle(ctx, event, data, env);
        self.shadow.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        let old_down = self.button.widget().is_down();
        self.button.update(ctx, data, env);

        // Because of the shadow, our paint rect is bigger than the button's paint rect, and we
        // need to make sure that we invalidated the bigger rect.
        if old_down != self.button.widget().is_down() {
            ctx.request_paint();
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        // piet draws a drop shadow 2.5 times the size of the radius.
        let shadow_insets = Insets::uniform(env.get(crate::DROP_SHADOW_RADIUS) * 2.5);
        self.button.widget_mut().set_insets(shadow_insets);

        let button_size = self.button.layout(ctx, bc, data, env);
        self.shadow
            .layout(ctx, &BoxConstraints::tight(button_size), data, env);
        self.button.set_origin(ctx, data, env, Point::ZERO);
        self.shadow.set_origin(ctx, data, env, Point::ZERO);
        ctx.set_paint_insets(self.shadow.paint_insets());
        button_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        if !self.button.widget().is_down() {
            self.shadow.paint(ctx, data, env);
        }
        self.button.paint(ctx, data, env);
    }
}
