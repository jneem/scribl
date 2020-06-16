use anyhow::{anyhow, Error};
use druid::kurbo::TranslateScale;
use druid::piet::{self, Device, ImageFormat, InterpolationMode, RenderContext};
use druid::{Color, Rect, Size};

use scribl_curves::{Cursor, SnippetId, SnippetsData, Time};

pub struct ImageBuf {
    width: usize,
    height: usize,
    buf: Vec<u8>,
    cursor: Cursor<Time, SnippetId>,
}

impl ImageBuf {
    pub fn new(width: usize, height: usize, snippets: &SnippetsData) -> ImageBuf {
        ImageBuf {
            width,
            height,
            buf: vec![255; width * height * 4],
            cursor: snippets.create_cursor(Time::ZERO),
        }
    }

    pub fn render(&mut self, snippets: &SnippetsData, time: Time) -> Result<(), Error> {
        let last_time = self.cursor.current().0;
        let mut bbox = Rect::ZERO;

        // TODO: we have a cursor for visible snippets, but we could also have a cursor for
        // snippets that might potentially cause a change in the visibility. There should be less
        // of these.
        self.cursor
            .advance_to(time.min(last_time), time.max(last_time));
        for b in self.cursor.bboxes(snippets) {
            let b = (TranslateScale::scale(self.width as f64) * b).expand();
            if bbox.area() == 0.0 {
                bbox = b;
            } else {
                // TODO: maybe if there are only a few bboxes, we should avoid unioning
                // them.
                bbox = bbox.union(b);
            }
        }
        if bbox.area() == 0.0 {
            return Ok(());
        }

        let mut device = Device::new().map_err(|e| anyhow!("failed to get device: {}", e))?;
        let mut bitmap = device
            .bitmap_target(self.width, self.height, 1.0)
            .map_err(|e| anyhow!("failed to get bitmap: {}", e))?;
        let mut ctx = bitmap.render_context();
        let old_image = self
            .make_image(&mut ctx)
            .map_err(|e| anyhow!("failed to make image: {}", e))?;
        ctx.draw_image(
            &old_image,
            Size::new(self.width as f64, self.height as f64).to_rect(),
            InterpolationMode::NearestNeighbor,
        );
        self.cursor.advance_to(time, time);
        ctx.with_save(|ctx| {
            ctx.clip(bbox);
            ctx.transform(TranslateScale::scale(self.width as f64).into());
            ctx.clear(Color::WHITE);
            for id in self.cursor.active_ids() {
                snippets.snippet(id).render(ctx, time);
            }
            Ok(())
        })
        .map_err(|e| anyhow!("failed to render: {}", e))?;
        ctx.finish()
            .map_err(|e| anyhow!("failed to finish context: {}", e))?;
        // Note that piet-cairo (and probably other backends too) currently only supports
        // RgbaPremul.
        self.buf = bitmap
            .into_raw_pixels(ImageFormat::RgbaPremul)
            .map_err(|e| anyhow!("failed to get raw pixels: {}", e))?;
        Ok(())
    }

    pub fn make_image<R: RenderContext>(&self, ctx: &mut R) -> Result<R::Image, piet::Error> {
        ctx.make_image(self.width, self.height, &self.buf, ImageFormat::RgbaPremul)
    }

    pub fn pixel_data(&self) -> &[u8] {
        &self.buf[..]
    }
}
