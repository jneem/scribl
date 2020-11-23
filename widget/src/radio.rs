use druid::widget::prelude::*;
use druid::widget::{Axis, LabelText};
use druid::{Data, Insets, Rect, WidgetPod};

use crate::{Icon, Shadow, ShadowlessToggleButton};

struct RadioButton<T> {
    button: WidgetPod<T, ShadowlessToggleButton<T>>,
    shadow: WidgetPod<T, Shadow>,
}

pub struct RadioGroup<T: Data> {
    children: Vec<RadioButton<T>>,
    axis: Axis,
    padding: f64,
}

impl<T: Data> RadioGroup<T> {
    fn new<I: IntoIterator<Item = ShadowlessToggleButton<T>>>(axis: Axis, children: I) -> Self {
        let mut buttons = Vec::new();
        for child in children {
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

    fn new_from_icons<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        axis: Axis,
        padding: f64,
        children: I,
    ) -> Self {
        Self::new(
            axis,
            children.into_iter().map(|(icon, variant, text)| {
                let variant_clone = variant.clone();
                ShadowlessToggleButton::<T>::from_icon(
                    icon,
                    padding,
                    text,
                    move |data| data.same(&variant),
                    move |_, state, _| {
                        *state = variant_clone.clone();
                    },
                    |_, _, _| {},
                )
            }),
        )
    }

    /// Creates a group of icon buttons in a row, with tooltips.
    pub fn icon_row<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        children: I,
        padding: f64,
    ) -> Self {
        Self::new_from_icons(Axis::Horizontal, padding, children)
    }

    /// Creates a group of icon buttons in a column, with tooltips.
    pub fn icon_column<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        children: I,
        padding: f64,
    ) -> Self {
        Self::new_from_icons(Axis::Vertical, padding, children)
    }

    /// Creates a group of buttons in a column, with custom widgets on the buttons.
    pub fn column<I: IntoIterator<Item = (Box<dyn Widget<T>>, T)>>(children: I) -> Self {
        Self::new(
            Axis::Vertical,
            children.into_iter().map(|(child, variant)| {
                let variant_clone = variant.clone();
                ShadowlessToggleButton::from_widget(
                    child,
                    move |data| data.same(&variant),
                    move |_, state, _| {
                        *state = variant_clone.clone();
                    },
                    |_, _, _| {},
                )
            }),
        )
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
            let old_down = c.button.widget().is_down();
            c.button.update(ctx, data, env);
            c.shadow.update(ctx, data, env);
            if old_down != c.button.widget().is_down() {
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
            c.button.widget_mut().set_insets(shadow_insets);
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
            if c.button.widget().is_down() {
                c.button.paint(ctx, data, env);
            }
        }
        for c in &mut self.children {
            if !c.button.widget().is_down() {
                c.shadow.paint(ctx, data, env);
            }
        }
        for c in &mut self.children {
            if !c.button.widget().is_down() {
                c.button.paint(ctx, data, env);
            }
        }
    }
}
