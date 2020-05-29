use druid::kurbo::{Affine, BezPath, Vec2};
use druid::widget::prelude::*;
use druid::widget::{Flex, LabelText, WidgetExt};
use druid::{theme, Data};

use crate::widgets::icons::Icon;
use crate::widgets::tooltip::TooltipExt;

pub struct RadioIcon<T: Data> {
    icon_size: Size,
    icon_scale: f64,
    icon_path: BezPath,
    variant: T,
}

pub fn make_radio_icon_group<'a, T: Data, I: IntoIterator<Item = (&'a Icon, T, LabelText<T>)>>(
    height: f64,
    children: I,
) -> impl Widget<T> {
    let mut group = Flex::row();
    for (icon, variant, text) in children {
        let icon_scale = height / icon.height as f64;
        let icon_width = icon.width as f64 * icon_scale;
        let child = RadioIcon {
            icon_size: Size::new(icon_width, height),
            icon_scale,
            icon_path: BezPath::from_svg(icon.path).unwrap(),
            variant,
        }
        .tooltip(text);
        group.add_child(child);
    }
    group
        .border(theme::BORDER_LIGHT, crate::BUTTON_GROUP_BORDER_WIDTH)
        // TODO: Get from the theme
        .rounded(5.0)
}

impl<T: Data> Widget<T> for RadioIcon<T> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, _env: &Env) {
        match ev {
            Event::MouseDown(_) => {
                ctx.set_active(true);
                ctx.request_paint();
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    if ctx.is_hot() {
                        *data = self.variant.clone();
                    }
                    ctx.request_paint();
                }
            }
            _ => {}
        }
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, _data: &T, _env: &Env) {
        if matches!(ev, LifeCycle::HotChanged(_)) {
            ctx.request_paint()
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, old_data: &T, data: &T, _env: &Env) {
        if !old_data.same(data) {
            ctx.request_paint()
        }
    }

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, _data: &T, env: &Env) -> Size {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let size = Size::new(
            self.icon_size.width + 2.0 * padding,
            self.icon_size.height + 2.0 * padding,
        );
        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let padding = env.get(crate::BUTTON_ICON_PADDING);
        let selected = data.same(&self.variant);
        let hot = ctx.is_hot();
        let icon_offset = Vec2::new(padding, padding);
        let icon_color = if selected {
            env.get(crate::BUTTON_ICON_SELECTED)
        } else if hot {
            env.get(crate::BUTTON_ICON_HOT)
        } else {
            env.get(crate::BUTTON_ICON_IDLE)
        };

        ctx.with_save(|ctx| {
            ctx.transform(Affine::translate(icon_offset) * Affine::scale(self.icon_scale));
            ctx.fill(&self.icon_path, &icon_color);
        });
    }
}
