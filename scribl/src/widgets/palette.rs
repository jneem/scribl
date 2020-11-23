use druid::widget::prelude::*;
use druid::{theme, Color, Data, Lens, RenderContext};
use std::sync::Arc;

use scribl_widget::{RadioGroup, TooltipExt};

#[derive(Clone, Data, Lens)]
pub struct PaletteData {
    colors: Arc<Vec<(Color, String)>>,
    selected: Color,
}

impl Default for PaletteData {
    fn default() -> PaletteData {
        // The utexas color palette defined here: https://brand.utexas.edu/identity/color/
        let colors = vec![
            (Color::rgb8(51, 63, 72), "Charcoal".to_owned()),
            (Color::rgb8(191, 87, 0), "Burnt orange".to_owned()),
            (Color::rgb8(248, 151, 31), "Kumquat".to_owned()),
            (Color::rgb8(255, 214, 0), "Golden".to_owned()),
            (Color::rgb8(166, 205, 87), "Yellow-green".to_owned()),
            (Color::rgb8(87, 157, 66), "May green".to_owned()),
            (Color::rgb8(0, 169, 183), "Cayman".to_owned()),
            (Color::rgb8(0, 95, 134), "Capri".to_owned()),
            (Color::rgb8(156, 173, 183), "Cadet".to_owned()),
            (Color::rgb8(214, 210, 196), "Timberwolf".to_owned()),
        ];
        let selected = colors[0].0.clone();
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

    pub fn try_select_idx(&mut self, idx: usize) -> Result<(), ()> {
        if let Some(c) = self.colors.get(idx) {
            self.selected = c.0.clone();
            Ok(())
        } else {
            Err(())
        }
    }
}

pub struct Palette {
    inner: RadioGroup<Color>,
}

pub struct PaletteElement {
    color: Color,
}

impl Widget<Color> for PaletteElement {
    fn event(&mut self, _ctx: &mut EventCtx, _event: &Event, _data: &mut Color, _env: &Env) {}
    fn update(&mut self, _ctx: &mut UpdateCtx, _old_data: &Color, _data: &Color, _env: &Env) {}
    fn lifecycle(&mut self, _: &mut LifeCycleCtx, _: &LifeCycle, _: &Color, _: &Env) {}

    fn layout(&mut self, _: &mut LayoutCtx, bc: &BoxConstraints, _: &Color, _: &Env) -> Size {
        let max = bc.max();
        let size = max.width.min(max.height);
        Size::new(size, size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _: &Color, env: &Env) {
        let rect = ctx
            .size()
            .to_rounded_rect(env.get(theme::BUTTON_BORDER_RADIUS));

        ctx.fill(&rect, &self.color);
    }
}

impl Palette {
    /// Creates a new palette in which the color swatches have dimensions `color_size`.
    /// `color_size` is also the width of this widget (the height depends on the number of colors).
    pub fn new() -> Palette {
        Palette {
            inner: RadioGroup::column(None),
        }
    }

    fn resize(&mut self, colors: &[(Color, String)]) {
        self.inner = RadioGroup::column(colors.iter().enumerate().map(|(i, (c, name))| {
            let elt = PaletteElement { color: c.clone() };
            let widget = if i <= 9 {
                Box::new(elt.tooltip(format!("{} ({})", name, (i + 1) % 10)))
                    as Box<dyn Widget<Color>>
            } else {
                Box::new(elt) as Box<dyn Widget<Color>>
            };
            (widget, c.clone())
        }));
    }
}

impl Widget<PaletteData> for Palette {
    fn event(&mut self, ctx: &mut EventCtx, event: &Event, data: &mut PaletteData, env: &Env) {
        self.inner.event(ctx, event, &mut data.selected, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &PaletteData,
        data: &PaletteData,
        env: &Env,
    ) {
        if data.colors != old_data.colors {
            self.resize(&data.colors);
            ctx.children_changed();
            ctx.request_paint();
        } else {
            self.inner
                .update(ctx, &old_data.selected, &data.selected, env);
        }
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
            ctx.children_changed();
            ctx.request_layout();
        }
        self.inner.lifecycle(ctx, event, &data.selected, env);
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &PaletteData,
        env: &Env,
    ) -> Size {
        self.inner.layout(ctx, bc, &data.selected, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &PaletteData, env: &Env) {
        self.inner.paint(ctx, &data.selected, env);
    }
}
