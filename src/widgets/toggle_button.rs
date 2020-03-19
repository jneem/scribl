use druid::theme;
use druid::widget::{Label, LabelText};
use druid::{
    Affine, BoxConstraints, Data, Env, Event, EventCtx, Insets, LayoutCtx, LifeCycle, LifeCycleCtx,
    LinearGradient, PaintCtx, Point, Rect, RenderContext, Size, UnitPoint, UpdateCtx, Widget,
};

// copy-paste from the Button source
const LABEL_INSETS: Insets = Insets::uniform_xy(8., 2.);

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
    label: Label<T>,
    label_size: Size,
    toggle_state: Box<dyn Fn(&T) -> ToggleButtonState + 'static>,
    toggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
    untoggle_action: Box<dyn Fn(&mut EventCtx, &mut T, &Env) + 'static>,
}

impl<T: Data> ToggleButton<T> {
    pub fn new(
        text: impl Into<LabelText<T>>,
        toggle_state: impl Fn(&T) -> ToggleButtonState + 'static,
        toggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
        untoggle_action: impl Fn(&mut EventCtx, &mut T, &Env) + 'static,
    ) -> ToggleButton<T> {
        ToggleButton {
            label: Label::new(text),
            label_size: Size::ZERO,
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

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, data: &T, env: &Env) {
        if let LifeCycle::HotChanged(_) = event {
            ctx.request_paint();
        }
        self.label.lifecycle(ctx, event, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, env: &Env) {
        self.label.update(ctx, old_data, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        // Copy-paste from Button
        let padding = Size::new(LABEL_INSETS.x_value(), LABEL_INSETS.y_value());
        let label_bc = bc.shrink(padding).loosen();
        self.label_size = self.label.layout(ctx, &label_bc, data, env);
        // HACK: to make sure we look okay at default sizes when beside a textbox,
        // we make sure we will have at least the same height as the default textbox.
        let min_height = env.get(theme::BORDERED_WIDGET_HEIGHT);

        bc.constrain(Size::new(
            self.label_size.width + padding.width,
            (self.label_size.height + padding.height).max(min_height),
        ))
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let is_active = ctx.is_active();
        let state = (self.toggle_state)(data);
        let is_toggled = state == ToggleButtonState::ToggledOn;
        let is_disabled = state.is_disabled();
        let is_hot = ctx.is_hot();
        let size = ctx.size();

        let rounded_rect = Rect::from_origin_size(Point::ORIGIN, size)
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        let gradient = if is_disabled {
            (
                env.get(crate::BUTTON_DISABLED),
                env.get(crate::BUTTON_DISABLED),
            )
        } else if is_toggled != is_active {
            (env.get(theme::BUTTON_LIGHT), env.get(theme::BUTTON_DARK))
        } else {
            (env.get(theme::BUTTON_DARK), env.get(theme::BUTTON_LIGHT))
        };
        let gradient = LinearGradient::new(UnitPoint::TOP, UnitPoint::BOTTOM, gradient);

        let border_color = if is_hot {
            env.get(theme::BORDER_LIGHT)
        } else {
            env.get(theme::BORDER_DARK)
        };

        ctx.stroke(
            rounded_rect,
            &border_color,
            env.get(theme::BUTTON_BORDER_WIDTH),
        );
        ctx.fill(rounded_rect, &gradient);

        let label_offset = (size.to_vec2() - self.label_size.to_vec2()) / 2.0;
        if let Err(e) = ctx.save() {
            log::error!("saving render context failed: {:?}", e);
            return;
        }

        ctx.transform(Affine::translate(label_offset));
        self.label.paint(ctx, data, env);

        if let Err(e) = ctx.restore() {
            log::error!("saving render context failed: {:?}", e);
            return;
        }
    }
}
