use druid::kurbo::{BezPath, Shape};
use druid::widget::prelude::*;
use druid::widget::Label;
use druid::{Color, Data, KeyOrValue, Point, Rect, WidgetPod};

pub struct LabelledContainer<T: Data> {
    inner: WidgetPod<T, Box<dyn Widget<T>>>,
    label: WidgetPod<T, Label<T>>,
    border_width: KeyOrValue<f64>,
    border_color: KeyOrValue<Color>,
    corner_radius: KeyOrValue<f64>,

    // TODO: is there a way to get this from the label? (during paint)
    text_size: Size,
}

impl<T: Data> LabelledContainer<T> {
    pub fn new<W: Widget<T> + 'static>(inner: W, text: &'static str) -> LabelledContainer<T> {
        LabelledContainer {
            inner: WidgetPod::new(Box::new(inner)),
            label: WidgetPod::new(Label::new(text).with_text_size(crate::TEXT_SIZE_SMALL)),
            border_width: 1.0.into(),
            border_color: Color::BLACK.into(),
            corner_radius: 0.0.into(),
            text_size: (0.0, 0.0).into(),
        }
    }

    pub fn border_width<S: Into<KeyOrValue<f64>>>(mut self, width: S) -> Self {
        self.border_width = width.into();
        self
    }

    pub fn border_color<S: Into<KeyOrValue<Color>>>(mut self, color: S) -> Self {
        self.border_color = color.into();
        self
    }

    pub fn corner_radius<S: Into<KeyOrValue<f64>>>(mut self, corner_radius: S) -> Self {
        self.corner_radius = corner_radius.into();
        self
    }
}

impl<T: Data> Widget<T> for LabelledContainer<T> {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut T, env: &Env) {
        self.inner.event(ctx, ev, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &T, env: &Env) {
        self.inner.lifecycle(ctx, ev, data, env);
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &T, data: &T, env: &Env) {
        self.inner.update(ctx, data, env);
    }

    fn layout(&mut self, ctx: &mut LayoutCtx, bc: &BoxConstraints, data: &T, env: &Env) -> Size {
        let border_width = self.border_width.resolve(env);
        let corner_radius = self.corner_radius.resolve(env);
        self.text_size = self.label.layout(ctx, bc, data, env);
        let top = self.text_size.height.max(border_width);

        let inner_bc = bc.shrink((2.0 * border_width, top + border_width));
        let inner_size = self.inner.layout(ctx, &inner_bc, data, env);
        let inner_origin = Point::new(border_width, top);
        self.label.set_layout_rect(
            self.text_size
                .to_rect()
                .with_origin((corner_radius * 2.0, 0.0)),
        );
        self.inner
            .set_layout_rect(Rect::from_origin_size(inner_origin, inner_size));

        // Also make sure to allocate enough width for the text, with a little horizontal padding.
        let inner_width = inner_size
            .width
            .max(4.0 * corner_radius + self.text_size.width);

        let size = Size::new(
            inner_width + 2.0 * border_width,
            inner_size.height + top + border_width,
        );
        let paint_insets = self.inner.compute_parent_paint_insets(size);
        ctx.set_paint_insets(paint_insets);
        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let border_width = self.border_width.resolve(env);
        let border_color = self.border_color.resolve(env);
        let corner_radius = self.corner_radius.resolve(env);

        let size = ctx.size();
        let border_rect = Rect::from_origin_size(
            (0.0, self.text_size.height / 2.0),
            (size.width, size.height - self.text_size.height / 2.0),
        )
        .inset(-border_width / 2.0)
        .to_rounded_rect(corner_radius);

        // We clip a rectangle out of the path, by drawing a clockwise outer rectangle and
        // a counter-clockwise inner rectangle. Clipping to this has the effect of clipping to
        // the complement of the inner rectangle.
        let r = self.label.layout_rect();
        let mut path: BezPath = ctx.size().to_rect().to_bez_path(0.1).collect();
        path.move_to((r.x0, r.y0));
        path.line_to((r.x0, r.y1));
        path.line_to((r.x1, r.y1));
        path.line_to((r.x1, r.y0));
        path.close_path();
        ctx.with_save(|ctx| {
            ctx.clip(path);
            ctx.stroke(border_rect, &border_color, border_width);
        });
        self.label.paint_with_offset(ctx, data, env);
        self.inner.paint_with_offset(ctx, data, env);
    }
}
