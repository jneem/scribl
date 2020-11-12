use druid::kurbo::BezPath;
use druid::widget::prelude::*;
use druid::{theme, Affine, Data, Insets, RenderContext, Size, Vec2};

use crate::Icon;

#[derive(Clone, Copy, Data, Debug, Eq, PartialEq)]
pub enum ToggleButtonState {
    ToggledOn,
    ToggledOff,
    Disabled,
}

impl ToggleButtonState {
    pub fn is_disabled(&self) -> bool {
        *self == ToggleButtonState::Disabled
    }
}

impl From<bool> for ToggleButtonState {
    fn from(b: bool) -> ToggleButtonState {
        if b {
            ToggleButtonState::ToggledOn
        } else {
            ToggleButtonState::ToggledOff
        }
    }
}

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
    icon_path: BezPath,
    // The native size of the icon.
    icon_size: Size,
    icon_scale: f64,
    layer: Layer,
    toggle_state: Box<dyn Fn(&T) -> ToggleButtonState + 'static>,
    toggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
    untoggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
}

impl<T: Data> ToggleButton<T> {
    pub fn new(
        icon: &Icon,
        toggle_state: impl Fn(&T) -> ToggleButtonState + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        let icon_size = Size::new(icon.width as f64, icon.height as f64);

        ToggleButton {
            icon_path: BezPath::from_svg(icon.path).unwrap(),
            icon_size,
            icon_scale: 1.0,
            layer: Layer::All,
            toggle_state: Box::new(toggle_state),
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
    }

    pub fn width(mut self, width: f64) -> Self {
        self.icon_scale = width / self.icon_size.width;
        self
    }

    pub fn height(mut self, height: f64) -> Self {
        self.icon_scale = height / self.icon_size.height;
        self
    }

    pub fn icon_size(&self) -> Size {
        self.icon_size * self.icon_scale
    }

    pub(crate) fn set_layer(&mut self, layer: Layer) {
        self.layer = layer;
    }
}

impl<T: Data> Widget<T> for ToggleButton<T> {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut T, env: &Env) {
        if (self.toggle_state)(data).is_disabled() {
            if ctx.is_active() {
                ctx.set_active(false);
                ctx.request_paint();
            }
            return;
        }

        match event {
            Event::MouseDown(_) => {
                ctx.set_active(true);
                ctx.request_paint();
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    ctx.request_paint();
                    let state = (self.toggle_state)(data);
                    if ctx.is_hot() {
                        match state {
                            ToggleButtonState::ToggledOn => (self.untoggle_action)(ctx, data, env),
                            ToggleButtonState::ToggledOff => (self.toggle_action)(ctx, data, env),
                            ToggleButtonState::Disabled => {}
                        }
                    }
                }
            }
            Event::MouseMove(_) => {}
            _ => {}
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, _data: &T, _env: &Env) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, _env: &Env) {
        if (self.toggle_state)(old_data) != (self.toggle_state)(data) {
            ctx.request_paint();
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, _data: &T, env: &Env) -> Size {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let shadow_offset = env.get(crate::DROP_SHADOW_OFFSET);
        let shadow_radius = env.get(crate::DROP_SHADOW_RADIUS);
        let size = (
            self.icon_size().width + padding * 2.0,
            self.icon_size().height + padding * 2.0,
        );

        ctx.set_paint_insets(Insets::uniform_xy(
            padding + shadow_radius + shadow_offset.x / 2.0,
            padding + shadow_radius + shadow_offset.y / 2.0,
        ));

        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let shadow_offset = env.get(crate::DROP_SHADOW_OFFSET);
        let shadow_radius = env.get(crate::DROP_SHADOW_RADIUS);
        let shadow_color = env.get(crate::DROP_SHADOW_COLOR);
        let button_color = env.get(crate::BUTTON_ICON_BUTTON_COLOR);

        let state = (self.toggle_state)(data);
        let is_disabled = state.is_disabled();
        let is_hot = ctx.is_hot();
        let is_toggled = state == ToggleButtonState::ToggledOn;
        let is_pressed = is_toggled || (ctx.is_active() && is_hot);
        let size = ctx.size();

        let icon_color = if is_disabled {
            env.get(crate::BUTTON_ICON_DISABLED_COLOR)
        } else if is_toggled {
            env.get(crate::BUTTON_ICON_SELECTED_COLOR)
        } else {
            env.get(crate::BUTTON_ICON_COLOR)
        };
        let stroke_color = env.get(crate::BUTTON_ICON_SELECTED_COLOR);

        let shadow_offset = if is_pressed {
            Vec2::new(0.0, 0.0)
        } else {
            shadow_offset.to_vec2()
        };
        let icon_offset = (size.to_vec2() - self.icon_size().to_vec2()) / 2.0 - shadow_offset / 2.0;
        let button_rect = (self.icon_size().to_rect() + icon_offset).inset(padding);
        let shadow_rect = button_rect + shadow_offset;
        let button_rect = button_rect.to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));
        let draw_bottom = self.layer == Layer::All || self.layer == Layer::Bottom;
        let draw_top = self.layer == Layer::All || self.layer == Layer::Top;
        let draw_shadow = self.layer == Layer::All || self.layer == Layer::Shadow;

        if !is_pressed && draw_shadow {
            ctx.blurred_rect(shadow_rect, shadow_radius, &shadow_color);
        }
        if (is_pressed && draw_bottom) || (!is_pressed && draw_top) {
            ctx.fill(button_rect, &button_color);
            ctx.with_save(|ctx| {
                ctx.transform(Affine::translate(icon_offset) * Affine::scale(self.icon_scale));
                ctx.fill(&self.icon_path, &icon_color);
                if is_hot {
                    ctx.stroke(&self.icon_path, &stroke_color, 4.0);
                }
            });
        }
    }
}
