use anyhow::{anyhow, Result};
use druid::piet::{ImageFormat, InterpolationMode, PietImage};
use druid::widget::prelude::*;
use druid::{Color, Data, Point, Rect, Vec2, WidgetPod};

/// A re-implementation of druid's container, supporting drop-shadows (but no borders because we
/// don't use them).
pub struct SunkenContainer<T, W> {
    inner: WidgetPod<T, W>,
    top_shadow: Option<PietImage>,
    bottom_shadow: Option<PietImage>,
    shadow_width: usize,
    shadow_height: usize,
    // Whenever the width changes, we redraw the shadows.
    last_width: f64,
}

impl<T: Data, W: Widget<T>> SunkenContainer<T, W> {
    pub fn new(child: W) -> SunkenContainer<T, W> {
        SunkenContainer {
            inner: WidgetPod::new(child),
            top_shadow: None,
            bottom_shadow: None,
            shadow_width: 0,
            shadow_height: 0,
            last_width: 0.0,
        }
    }

    pub fn child(&self) -> &W {
        self.inner.widget()
    }

    pub fn child_mut(&mut self) -> &mut W {
        self.inner.widget_mut()
    }

    fn recreate_bitmaps_if_needed(
        &mut self,
        ctx: &mut PaintCtx,
        width: f64,
        radius: f64,
        color: &Color,
    ) -> Result<()> {
        if width == self.last_width {
            return Ok(());
        }

        self.last_width = width;
        // The 2.5 is a magic number from piet. It gives the actual drawn size of a blur.
        let shadow_rect =
            Rect::from_origin_size((-2.5 * radius, 0.0), (width + 5.0 * radius, 2.5 * radius));
        self.shadow_width = width.ceil() as usize;
        self.shadow_height = (radius * 2.5).ceil() as usize;
        let mut dev =
            druid::piet::Device::new().map_err(|e| anyhow!("error creating device: {}", e))?;

        {
            let mut top_bitmap = dev
                .bitmap_target(self.shadow_width, self.shadow_height, 1.0)
                .map_err(|e| anyhow!("error creating bitmap: {}", e))?;
            {
                let mut bmp_ctx = top_bitmap.render_context();
                bmp_ctx.blurred_rect(
                    shadow_rect - Vec2::new(0.0, shadow_rect.height()),
                    radius,
                    color,
                );
                bmp_ctx
                    .finish()
                    .map_err(|e| anyhow!("error finishing, {}", e))?;
            }
            let top_shadow = top_bitmap
                .to_image_buf(ImageFormat::RgbaPremul)
                .map_err(|e| anyhow!("error getting image: {}", e))?
                .to_image(ctx.render_ctx);
            self.top_shadow = Some(top_shadow);
        }

        {
            let mut bottom_bitmap = dev
                .bitmap_target(self.shadow_width, self.shadow_height, 1.0)
                .map_err(|e| anyhow!("error creating bitmap: {}", e))?;
            {
                let mut bmp_ctx = bottom_bitmap.render_context();
                bmp_ctx.blurred_rect(
                    shadow_rect + Vec2::new(0.0, shadow_rect.height()),
                    radius,
                    color,
                );
                bmp_ctx
                    .finish()
                    .map_err(|e| anyhow!("error finishing: {}", e))?;
            }
            let bottom_shadow = bottom_bitmap
                .to_image_buf(ImageFormat::RgbaPremul)
                .map_err(|e| anyhow!("error getting image: {}", e))?
                .to_image(ctx.render_ctx);
            self.bottom_shadow = Some(bottom_shadow);
        }
        Ok(())
    }
}

impl<T: Data, W: Widget<T>> Widget<T> for SunkenContainer<T, W> {
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
        let child_size = self.inner.layout(ctx, bc, data, env);
        self.inner.set_origin(ctx, data, env, Point::ZERO);
        child_size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &T, env: &Env) {
        let radius = env.get(crate::DROP_SHADOW_RADIUS);
        let color = env.get(crate::DROP_SHADOW_COLOR);
        self.inner.paint(ctx, data, env);

        let size = ctx.size();
        if let Err(e) = self.recreate_bitmaps_if_needed(ctx, size.width, radius, &color) {
            log::warn!("failed to create bitmaps, ignoring shadow: {}", e);
            return;
        }

        ctx.with_save(|ctx| {
            ctx.clip(size.to_rect());
            let top_rect = Size::new(self.shadow_width as f64, self.shadow_height as f64).to_rect();
            let bottom_rect = top_rect + Vec2::new(0.0, size.height - self.shadow_height as f64);

            if let Some(top) = &self.top_shadow {
                ctx.draw_image(top, top_rect, InterpolationMode::NearestNeighbor);
            }
            if let Some(bottom) = &self.bottom_shadow {
                ctx.draw_image(&bottom, bottom_rect, InterpolationMode::NearestNeighbor);
            }
        });
    }
}
