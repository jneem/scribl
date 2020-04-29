use druid::kurbo::Circle;
use druid::widget::prelude::*;
use druid::{Color, Command, Data, Lens, Point, Rect, RenderContext, WidgetPod};
use std::sync::Arc;

use crate::cmd;

const PALETTE_ELT_MIN_SIZE: f64 = 32.0;
const PALETTE_ELT_PADDING: f64 = 4.0;
const PALETTE_ROWS: u32 = 1;

#[derive(Clone, Data, Lens)]
pub struct PaletteData {
    colors: Arc<Vec<Color>>,
    selected: Color,
}

impl Default for PaletteData {
    fn default() -> PaletteData {
        let colors = vec![
            Color::rgb8(51, 63, 72),
            Color::rgb8(191, 87, 0),
            Color::rgb8(248, 151, 31),
            Color::rgb8(255, 214, 0),
            Color::rgb8(166, 205, 87),
            Color::rgb8(87, 157, 66),
            Color::rgb8(0, 169, 183),
            Color::rgb8(0, 95, 134),
            Color::rgb8(156, 173, 183),
            Color::rgb8(214, 210, 196),
        ];
        let selected = colors[0].clone();
        PaletteData {
            colors: Arc::new(colors),
            selected,
        }
    }
}

impl PaletteData {
    pub fn selected_color(&self) -> &Color {
        &self.selected
    }

    pub fn select(&mut self, color: &Color) {
        self.selected = color.clone();
    }
}

#[derive(Default)]
pub struct Palette {
    // The idiomatic thing to do would be to wrap the children in lenses, but the combinators
    // are hard to use for this since Vec doesn't implement Data.
    children: Vec<WidgetPod<Color, PaletteElement>>,
}

pub struct PaletteElement {
    color: Color,
}

impl Widget<Color> for PaletteElement {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, _data: &mut Color, _env: &Env) {
        match event {
            Event::MouseDown(_) => {
                ctx.set_active(true);
            }
            Event::MouseUp(_) => {
                if ctx.is_active() {
                    ctx.set_active(false);
                    ctx.submit_command(Command::new(cmd::CHOOSE_COLOR, self.color.clone()), None);
                }
            }
            _ => {}
        }
    }

    fn update(&mut self, ctx: &mut UpdateCtx, _old_data: &Color, _data: &Color, _env: &Env) {
        ctx.request_paint();
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, event: &LifeCycle, _data: &Color, _env: &Env) {
        match event {
            LifeCycle::HotChanged(_) => {
                ctx.request_paint();
            }
            _ => {}
        }
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &Color,
        _env: &Env,
    ) -> Size {
        bc.max()
    }

    fn paint(&mut self, ctx: &mut PaintCtx, selected_color: &Color, _env: &Env) {
        let is_selected = selected_color.as_rgba_u32() == self.color.as_rgba_u32();
        let rect = Rect::from_origin_size(Point::ORIGIN, ctx.size());

        ctx.fill(&rect, &self.color);
        if is_selected {
            ctx.stroke(&rect, &Color::BLACK, 1.0);

            // Draw a dot in the middle of the element, with the inverted color.
            let inv_color = !self.color.as_rgba_u32() | 0xFF;
            let inv_color = Color::from_rgba32_u32(inv_color);
            let dot = Circle::new(rect.center(), rect.width() / 6.0);
            ctx.fill(dot, &inv_color);
        } else if ctx.is_hot() {
            ctx.stroke(&rect, &Color::WHITE, 1.0);
        }
    }
}

impl Palette {
    fn resize(&mut self, colors: &[Color]) {
        self.children.resize_with(colors.len(), || {
            WidgetPod::new(PaletteElement {
                color: Color::BLACK,
            })
        });
        for (i, c) in colors.iter().enumerate() {
            self.children[i].widget_mut().color = c.clone();
        }
    }
}

impl Widget<PaletteData> for Palette {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut PaletteData, env: &Env) {
        for (i, c) in self.children.iter_mut().enumerate() {
            let mut color = (&data.colors)[i].clone();
            c.event(ctx, event, &mut color, env);
        }
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        _old_data: &PaletteData,
        data: &PaletteData,
        _env: &Env,
    ) {
        self.resize(&data.colors);
        ctx.children_changed();
        ctx.request_paint();
    }

    fn lifecycle(
        &mut self,
        ctx: &mut LifeCycleCtx,
        event: &LifeCycle,
        data: &PaletteData,
        env: &Env,
    ) {
        if let LifeCycle::WidgetAdded = event {
            self.resize(&data.colors);
            ctx.request_layout();
        }
        for (i, c) in self.children.iter_mut().enumerate() {
            c.lifecycle(ctx, event, &(&data.colors)[i], env);
        }
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteData,
        env: &Env,
    ) -> Size {
        let rows = PALETTE_ROWS;
        // The (+ rows / 2) part means the columns round up. (and it works even if rows == 1)
        let cols = ((data.colors.len() as u32) + rows / 2) / PALETTE_ROWS;
        let min_height =
            PALETTE_ELT_MIN_SIZE * rows as f64 + PALETTE_ELT_PADDING * (rows - 1) as f64;
        let min_width =
            PALETTE_ELT_MIN_SIZE * cols as f64 + PALETTE_ELT_PADDING * (cols - 1) as f64;
        let size = bc.constrain(Size::new(min_width, min_height));

        // Note that we don't actually call layout on the children. I hope this isn't a problem...

        let actual_child_width =
            (size.width - (PALETTE_ELT_PADDING * (cols - 1) as f64)) / cols as f64;
        let actual_child_height =
            (size.height - (PALETTE_ELT_PADDING * (rows - 1) as f64)) / rows as f64;
        let actual_child_size = Size::new(actual_child_width, actual_child_height);
        for (i, c) in self.children.iter_mut().enumerate() {
            let i = i as u32;
            let row = i % PALETTE_ROWS;
            let col = i / PALETTE_ROWS;
            let x = actual_child_width * col as f64 + PALETTE_ELT_PADDING * col as f64;
            let y = actual_child_height * row as f64 + PALETTE_ELT_PADDING * row as f64;
            c.set_layout_rect(
                ctx,
                &data.colors[i as usize],
                env,
                Rect::from_origin_size((x, y), actual_child_size),
            );
        }

        size
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteData, env: &Env) {
        for c in &mut self.children {
            c.paint_with_offset(ctx, &data.selected_color(), env);
        }
    }
}
