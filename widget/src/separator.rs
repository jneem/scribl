use druid::widget::prelude::*;
use druid::{Color, KeyOrValue};

pub struct Separator {
    // If any dimensions are infinite, that means we will be as big as possible in that direction.
    size: Size,
    color: KeyOrValue<Color>,
}

impl Separator {
    pub fn new() -> Separator {
        Separator {
            size: Size::new(f64::INFINITY, f64::INFINITY),
            color: druid::theme::BACKGROUND_LIGHT.into(),
        }
    }

    pub fn width(mut self, width: f64) -> Separator {
        self.size.width = width;
        self
    }

    pub fn height(mut self, height: f64) -> Separator {
        self.size.height = height;
        self
    }

    pub fn color(mut self, color: impl Into<KeyOrValue<Color>>) -> Separator {
        self.color = color.into();
        self
    }
}

impl<T> Widget<T> for Separator {
    fn event(&mut self, _: &mut EventCtx, _: &Event, _: &mut T, _: &Env) {}
    fn lifecycle(&mut self, _: &mut LifeCycleCtx, _: &LifeCycle, _: &T, _: &Env) {}
    fn update(&mut self, _: &mut UpdateCtx, _: &T, _: &T, _: &Env) {}

    fn layout(&mut self, _ctx: &mut LayoutCtx, bc: &BoxConstraints, _: &T, _env: &Env) -> Size {
        bc.constrain(self.size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _: &T, env: &Env) {
        let color = self.color.resolve(env);
        let rect = ctx.size().to_rect();
        ctx.fill(&rect, &color);
    }
}
