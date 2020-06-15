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

    pub fn render(&mut self, snippets: &SnippetsData, time: Time) {
        let last_time = self.cursor.current().0;
        let mut bbox = Rect::ZERO;

        // TODO: we have a cursor for visible snippets, but we could also have a cursor for
        // snippets that might potentially cause a change in the visibility. There should be less
        // of these.
        self.cursor
            .advance_to(time.min(last_time), time.max(last_time));
        for snip in self.cursor.active_ids().map(|id| snippets.snippet(id)) {
            let last_local_time = snip.lerp().unlerp_clamped(self.cursor.current().0);
            let local_time = snip.lerp().unlerp_clamped(time);
            let (last_local_time, local_time) = (
                last_local_time.min(local_time),
                last_local_time.max(local_time),
            );
            // TODO: this is linear in the number of strokes, but probably most strokes will be
            // uninteresting. Using some extra cached computations in SnippetData, this could be
            // made (linear in useful strokes + logarithmic in total strokes).
            for stroke in snip.strokes() {
                let b = (TranslateScale::scale(self.width as f64)
                    * stroke.changes_bbox(last_local_time, local_time))
                .expand();
                if b.area() != 0.0 {
                    if bbox.area() == 0.0 {
                        bbox = b;
                    } else {
                        // TODO: maybe if there are only a few bboxes, we should avoid unioning
                        // them.
                        bbox = bbox.union(b);
                    }
                }
            }
        }
        if bbox.area() == 0.0 {
            return;
        }

        let mut device = Device::new().unwrap(); // FIXME
        let mut bitmap = device.bitmap_target(self.width, self.height, 1.0).unwrap(); // FIXME
        let mut ctx = bitmap.render_context();
        let old_image = self.make_image(&mut ctx).unwrap();
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
        .unwrap();
        ctx.finish().unwrap();
        // Note that piet-cairo (and probably other backends too) currently only supports
        // RgbaPremul.
        self.buf = bitmap.into_raw_pixels(ImageFormat::RgbaPremul).unwrap();
    }

    pub fn make_image<R: RenderContext>(&self, ctx: &mut R) -> Result<R::Image, piet::Error> {
        ctx.make_image(self.width, self.height, &self.buf, ImageFormat::RgbaPremul)
    }

    pub fn pixel_data(&self) -> &[u8] {
        &self.buf[..]
    }
}
