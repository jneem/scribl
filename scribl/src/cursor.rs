use anyhow::{anyhow, Error};
use druid::kurbo::BezPath;
use druid::piet::{Device, ImageFormat};
use druid::{Color, Cursor, CursorDesc, ImageBuf, RenderContext, WindowHandle};
use std::collections::HashMap;

use crate::widgets::icons;

pub struct CursorCache {
    size: u32,
    /// Maps from the RGBA color to a cursor.
    pens: HashMap<u32, Cursor>,
}

fn make_pen(window: &WindowHandle, color: &Color, size: u32) -> Cursor {
    fn inner(window: &WindowHandle, color: &Color, size: u32) -> Result<Cursor, Error> {
        let mut device = Device::new().map_err(|e| anyhow!("failed to get device: {}", e))?;
        let mut bitmap = device
            .bitmap_target(size as usize, size as usize, 1.0)
            .map_err(|e| anyhow!("failed to make bitmap: {}", e))?;
        let path = BezPath::from_svg(&icons::PEN.path).unwrap();
        {
            let mut ctx = bitmap.render_context();
            ctx.fill(&path, color);
            ctx.stroke(&path, &Color::BLACK, 2.0);
        }
        let image = bitmap
            .raw_pixels(ImageFormat::RgbaPremul)
            .map_err(|e| anyhow!("failed to get pixels: {}", e))?;
        let image =
            ImageBuf::from_raw(image, ImageFormat::RgbaPremul, size as usize, size as usize);
        let cursor_desc = CursorDesc::new(image, (1.0, 1.0));
        window
            .make_cursor(&cursor_desc)
            .ok_or(anyhow!("failed to make cursor"))
    }

    match inner(window, color, size) {
        Ok(c) => c,
        Err(e) => {
            log::error!("failed to create cursor: {}", e);
            Cursor::Arrow
        }
    }
}

impl CursorCache {
    pub fn new(size: u32) -> CursorCache {
        CursorCache {
            size,
            pens: HashMap::new(),
        }
    }

    pub fn pen(&mut self, window: &WindowHandle, color: &Color) -> &Cursor {
        let color_u32 = color.as_rgba_u32();
        let size = self.size;
        self.pens
            .entry(color_u32)
            .or_insert_with(|| make_pen(window, color, size))
    }
}
