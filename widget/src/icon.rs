use druid::kurbo::{Affine, BezPath};
use druid::widget::prelude::*;
use druid::widget::BackgroundBrush;
use druid::{Data, Size, Widget};

/// An icon made up of a single path (which should be filled with whatever color we want).
pub struct Icon {
    /// The width of the icon.
    pub width: u32,
    /// The height of the icon.
    pub height: u32,
    /// The icon's path, in SVG format.
    pub path: &'static str,
}

impl Icon {
    pub fn to_widget<T>(&self, brush: impl Into<BackgroundBrush<T>>) -> IconWidget<T> {
        IconWidget::from_icon(self, brush)
    }
}

pub struct IconWidget<T> {
    path: BezPath,
    // The size of the icon. Think of it as the bounding box of the path, but it isn't necessarily
    // exactly that.
    size: Size,
    brush: BackgroundBrush<T>,
}

impl<T> IconWidget<T> {
    /// Creates a new `IconWidget` for displaying an `Icon`.
    pub fn from_icon(icon: &Icon, brush: impl Into<BackgroundBrush<T>>) -> Self {
        IconWidget {
            path: BezPath::from_svg(icon.path).unwrap(),
            size: Size::new(icon.width as f64, icon.height as f64),
            brush: brush.into(),
        }
    }

    /// Scales this icon to have a given width (while preserving the aspect ratio).
    pub fn width(mut self, width: f64) -> Self {
        self.set_width(width);
        self
    }

    /// Scales this icon to have a given height (while preserving the aspect ratio).
    pub fn height(mut self, height: f64) -> Self {
        self.set_height(height);
        self
    }

    pub fn set_width(&mut self, width: f64) {
        self.scale_by(width / self.size.width);
    }

    pub fn set_height(&mut self, height: f64) {
        self.scale_by(height / self.size.height);
    }

    fn scale_by(&mut self, factor: f64) {
        self.size *= factor;
        self.path.apply_affine(Affine::scale(factor));
    }
}

impl<T: Data> Widget<T> for IconWidget<T> {
    fn event(&mut self, _: &mut EventCtx, _: &Event, _: &mut T, _: &Env) {}
    fn lifecycle(&mut self, _: &mut LifeCycleCtx, _: &LifeCycle, _: &T, _: &Env) {}
    fn update(&mut self, ctx: &mut UpdateCtx, _: &T, _: &T, _: &Env) {
        ctx.request_paint();
    }

    fn layout(&mut self, _: &mut LayoutCtx, bc: &BoxConstraints, _: &T, _: &Env) -> Size {
        bc.constrain(self.size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        ctx.with_save(|ctx| {
            ctx.clip(&self.path);
            self.brush.paint(ctx, data, env);
        });
    }
}
