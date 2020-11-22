use druid::widget::prelude::*;
use druid::Insets;

/// A widget that just draws a rectangular shadow.
#[derive(Debug)]
pub struct Shadow;

impl<T> Widget<T> for Shadow {
    fn event(&mut self, _: &mut EventCtx, _: &Event, _: &mut T, _: &Env) {}
    fn lifecycle(&mut self, _: &mut LifeCycleCtx, _: &LifeCycle, _: &T, _: &Env) {}
    fn update(&mut self, _: &mut UpdateCtx, _: &T, _: &T, _: &Env) {}

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, _: &T, env: &Env) -> Size {
        let radius = env.get(crate::DROP_SHADOW_RADIUS);
        ctx.set_paint_insets(Insets::uniform(radius));
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _: &T, env: &Env) {
        let radius = env.get(crate::DROP_SHADOW_RADIUS);
        let color = env.get(crate::DROP_SHADOW_COLOR);
        let rect = ctx.size().to_rect();
        ctx.blurred_rect(rect, radius, &color);
    }
}
