use druid::kurbo::BezPath;
use druid::theme;
use druid::{
    Affine, BoxConstraints, Data, Env, Event, EventCtx, LayoutCtx, LifeCycle, LifeCycleCtx,
    LinearGradient, PaintCtx, Point, Rect, RenderContext, Size, UnitPoint, UpdateCtx, Widget,
};

use crate::widgets::Icon;

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

pub struct ToggleButton<T> {
    icon_path: BezPath,
    icon_size: Size,
    icon_scale: f64,
    toggle_state: Box<dyn Fn(&T) -> ToggleButtonState + 'static>,
    toggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
    untoggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
}

impl<T: Data> ToggleButton<T> {
    pub fn new(
        icon: &Icon,
        icon_height: f64,
        toggle_state: impl Fn(&T) -> ToggleButtonState + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        let icon_scale = icon_height / icon.height as f64;
        let icon_width = icon.width as f64 * icon_scale;
        ToggleButton {
            icon_path: BezPath::from_svg(icon.path).unwrap(),
            icon_size: (icon_width, icon_height).into(),
            icon_scale,
            toggle_state: Box::new(toggle_state),
            toggle_action: Box::new(toggle_action),
            untoggle_action: Box::new(untoggle_action),
        }
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
            Event::MouseMoved(_) => {}
            e => {
                dbg!(e);
            }
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, _data: &T, _env: &Env) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, _env: &Env) {
        if !old_data.same(data) {
            ctx.request_paint();
        }
    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, _data: &T, env: &Env) -> Size {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let border_width = env.get(theme::BUTTON_BORDER_WIDTH);
        let size = (
            self.icon_size.width + padding * 2.0 + border_width * 2.0,
            self.icon_size.height + padding * 2.0 + border_width * 2.0,
        );
        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let is_active = ctx.is_active();
        let state = (self.toggle_state)(data);
        let is_toggled = state == ToggleButtonState::ToggledOn;
        let is_disabled = state.is_disabled();
        let is_hot = ctx.is_hot();
        let size = ctx.size();
        let border_width = env.get(theme::BUTTON_BORDER_WIDTH);

        let rounded_rect = Rect::from_origin_size(Point::ORIGIN, size)
            .inset(-border_width / 2.0)
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        let gradient = if is_disabled {
            (
                env.get(crate::BUTTON_BACKGROUND_DISABLED),
                env.get(crate::BUTTON_BACKGROUND_DISABLED),
            )
        } else if is_toggled != is_active {
            (env.get(theme::BUTTON_LIGHT), env.get(theme::BUTTON_DARK))
        } else {
            (env.get(theme::BUTTON_DARK), env.get(theme::BUTTON_LIGHT))
        };
        let gradient = LinearGradient::new(UnitPoint::TOP, UnitPoint::BOTTOM, gradient);

        let border_color = if is_hot && !is_disabled {
            env.get(theme::BORDER_LIGHT)
        } else {
            env.get(theme::BORDER_DARK)
        };

        let icon_color = if is_disabled {
            env.get(crate::BUTTON_FOREGROUND_DISABLED)
        } else {
            env.get(theme::FOREGROUND_LIGHT)
        };

        ctx.stroke(rounded_rect, &border_color, border_width);
        ctx.fill(rounded_rect, &gradient);

        let icon_offset = (size.to_vec2() - self.icon_size.to_vec2()) / 2.0;
        ctx.with_save(|ctx| {
            ctx.transform(Affine::translate(icon_offset) * Affine::scale(self.icon_scale));
            ctx.fill(&self.icon_path, &icon_color);
        });
    }
}
