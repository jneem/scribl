use druid::widget::prelude::*;
use druid::widget::{Axis, LabelText};
use druid::{Data, Insets, Rect, WidgetPod};

use crate::{Icon, Shadow, ShadowlessToggleButton, TooltipExt, TooltipHost};

struct RadioButton<T> {
    button: WidgetPod<T, TooltipHost<T, ShadowlessToggleButton<T>>>,
    shadow: WidgetPod<T, Shadow>,
}

pub struct RadioGroup<T: Data> {
    children: Vec<RadioButton<T>>,
    axis: Axis,
    padding: f64,
}

impl<T: Data> RadioGroup<T> {
    fn new<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        axis: Axis,
        size: f64,
        children: I,
    ) -> Self {
        let mut buttons = Vec::new();
        for (icon, variant, text) in children {
            let variant_clone = variant.clone();
            let child = ShadowlessToggleButton::<T>::from_icon(
                icon,
                move |data| data.same(&variant),
                move |_, state, _| {
                    *state = variant_clone.clone();
                },
                |_, _, _| {},
            );
            let child = if axis == Axis::Horizontal {
                child.icon_height(size)
            } else {
                child.icon_width(size)
            };
            let child = child.tooltip(text);

            buttons.push(RadioButton {
                button: WidgetPod::new(child),
                shadow: WidgetPod::new(Shadow),
            });
        }

        RadioGroup {
            children: buttons,
            axis,
            padding: 1.0,
        }
    }

    /// Creates a group of buttons in a row.
    ///
    /// `height` is the height of the icon contained in the buttons.
    pub fn row<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        height: f64,
        children: I,
    ) -> Self {
        Self::new(Axis::Horizontal, height, children)
    }

    /// Creates a group of buttons in a column.
    pub fn column<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        width: f64,
        children: I,
    ) -> Self {
        Self::new(Axis::Vertical, width, children)
    }
}

impl<T: Data> Widget<T> for RadioGroup<T> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        for c in &mut self.children {
            c.button.event(ctx, ev, data, env);
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        for c in &mut self.children {
            c.button.lifecycle(ctx, ev, data, env);
            c.shadow.lifecycle(ctx, ev, data, env);
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        for c in &mut self.children {
            let old_down = c.button.widget().child().is_down();
            c.button.update(ctx, data, env);
            c.shadow.update(ctx, data, env);
            if old_down != c.button.widget().child().is_down() {
                ctx.request_paint();
            }
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let shadow_insets = Insets::uniform(env.get(crate::DROP_SHADOW_RADIUS));
        let mut major_size = 0.0f64;
        let mut minor_size = 0.0f64;
        let mut paint_rect = Rect::ZERO;
        for c in &mut self.children {
            let size = Size::from(self.axis.pack(major_size, 0.0));
            let child_bc = bc.shrink(size);
            c.button.widget_mut().child_mut().set_insets(shadow_insets);
            let child_size = c.button.layout(ctx, &child_bc, data, env);
            let child_origin = size.to_vec2().to_point();
            c.button.set_origin(ctx, data, env, child_origin);
            c.shadow
                .layout(ctx, &BoxConstraints::tight(child_size), data, env);
            c.shadow.set_origin(ctx, data, env, child_origin);
            paint_rect = paint_rect.union(c.shadow.paint_rect());

            major_size += self.padding + self.axis.major(child_size);
            minor_size = minor_size.max(self.axis.minor(child_size));
        }
        let size = bc.constrain(Size::from(self.axis.pack(major_size, minor_size)));
        ctx.set_paint_insets(paint_rect - size.to_rect());

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        for c in &mut self.children {
            if c.button.widget().child().is_down() {
                c.button.paint(ctx, data, env);
            }
        }
        for c in &mut self.children {
            if !c.button.widget().child().is_down() {
                c.shadow.paint(ctx, data, env);
            }
        }
        for c in &mut self.children {
            if !c.button.widget().child().is_down() {
                c.button.paint(ctx, data, env);
            }
        }
    }
}
