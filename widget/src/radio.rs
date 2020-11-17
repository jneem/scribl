use druid::widget::prelude::*;
use druid::widget::{Axis, LabelText};
use druid::{Data, Rect, WidgetPod};

use crate::{Icon, ToggleButton, ToggleButtonState, TooltipExt, TooltipHost};

pub struct RadioGroup<T: Data> {
    children: Vec<WidgetPod<T, TooltipHost<T, ToggleButton<T>>>>,
    axis: Axis,
    padding: f64,
}

impl<T: Data> RadioGroup<T> {
    fn new<'a, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
        axis: Axis,
        cross_axis_size: f64,
        children: I,
    ) -> Self {
        let mut children_pods = Vec::new();
        for (icon, variant, text) in children {
            let variant_clone = variant.clone();
            let child = ToggleButton::<T>::new(
                icon,
                move |data| {
                    if data.same(&variant) {
                        ToggleButtonState::ToggledOn
                    } else {
                        ToggleButtonState::ToggledOff
                    }
                },
                move |_, state, _| {
                    *state = variant_clone.clone();
                },
                |_, _, _| {},
            );

            let child = if axis == Axis::Vertical {
                child.width(cross_axis_size)
            } else {
                child.height(cross_axis_size)
            }
            .tooltip(text);

            children_pods.push(WidgetPod::new(child));
        }

        RadioGroup {
            children: children_pods,
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
            c.event(ctx, ev, data, env);
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        for c in &mut self.children {
            c.lifecycle(ctx, ev, data, env);
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        for c in &mut self.children {
            c.update(ctx, data, env);
        }
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let mut major_size = 0.0f64;
        let mut minor_size = 0.0f64;
        let mut paint_rect = Rect::ZERO;
        for c in &mut self.children {
            let size = Size::from(self.axis.pack(major_size, 0.0));
            let child_bc = bc.shrink(size);
            let child_size = c.layout(ctx, &child_bc, data, env);
            c.set_layout_rect(ctx, data, env, child_size.to_rect() + size.to_vec2());
            paint_rect = paint_rect.union(c.paint_rect());

            major_size += self.padding + self.axis.major(child_size);
            minor_size = minor_size.max(self.axis.minor(child_size));
        }
        let size = bc.constrain(Size::from(self.axis.pack(major_size, minor_size)));
        ctx.set_paint_insets(paint_rect - size.to_rect());

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        for c in &mut self.children {
            c.widget_mut()
                .child_mut()
                .set_layer(crate::toggle_button::Layer::Bottom);
            c.paint(ctx, data, env);
        }
        for c in &mut self.children {
            c.widget_mut()
                .child_mut()
                .set_layer(crate::toggle_button::Layer::Shadow);
            c.paint(ctx, data, env);
        }
        for c in &mut self.children {
            c.widget_mut()
                .child_mut()
                .set_layer(crate::toggle_button::Layer::Top);
            c.paint(ctx, data, env);
        }
    }
}
