use druid::widget::prelude::*;
use druid::widget::Painter;
use druid::{Color, Point, Rect};

use scribl_widget::IconWidget;

use crate::widgets::icons::MICROPHONE;
use crate::EditorState;

static BAR_COLORS: &[Color] = &[
    Color::rgb8(166, 205, 87),
    Color::rgb8(166, 205, 87),
    Color::rgb8(255, 214, 0),
    Color::rgb8(255, 214, 0),
    Color::rgb8(248, 151, 31),
    Color::rgb8(248, 151, 31),
];

pub struct AudioIndicator {
    icon: IconWidget<EditorState>,
}

fn calc_bands(loudness: f64) -> usize {
    // 0.0 or above means all the bands. Then we do 1 band per 10 dB.
    BAR_COLORS
        .len()
        .saturating_sub(((-loudness).max(0.0) / 10.0).floor() as usize)
}

pub fn audio_indicator() -> Painter<EditorState> {
    Painter::new(|ctx, data: &EditorState, env| {
        let bands = calc_bands(data.input_loudness);
        let background = if data.action.is_recording_audio() {
            scribl_widget::BUTTON_ICON_SELECTED_COLOR
        } else {
            scribl_widget::BUTTON_ICON_COLOR
        };
        let rect = ctx.size().to_rect();
        let rect = ctx
            .current_transform()
            .inverse()
            .transform_rect_bbox(rect)
            .with_origin(Point::ZERO);

        ctx.fill(rect, &env.get(background));
        if data.action.is_recording_audio() {
            let band_height = rect.height() / (BAR_COLORS.len() as f64 * 1.5);
            let band_offset = band_height * 1.5;

            for i in 0..bands {
                let bottom = rect.height() - i as f64 * band_offset;
                let rect = Rect::new(0.0, bottom - band_height, rect.width(), bottom);
                ctx.fill(rect, &BAR_COLORS[i]);
            }
        }
    })
}

impl AudioIndicator {
    pub fn new() -> AudioIndicator {
        AudioIndicator {
            icon: IconWidget::from_icon(&MICROPHONE, audio_indicator()),
        }
    }
}

impl Widget<EditorState> for AudioIndicator {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut EditorState, env: &Env) {
        self.icon.event(ctx, ev, data, env);
    }

    fn lifecycle(&mut self, ctx: &mut LifeCycleCtx, ev: &LifeCycle, data: &EditorState, env: &Env) {
        self.icon.lifecycle(ctx, ev, data, env);
    }

    fn update(
        &mut self,
        ctx: &mut UpdateCtx,
        old_data: &EditorState,
        data: &EditorState,
        _env: &Env,
    ) {
        if calc_bands(data.input_loudness) != calc_bands(old_data.input_loudness) {
            ctx.request_paint();
        }
        // Don't bother calling update on the icon, because it all it ever does is request paint
        // (and we just did a more accurate check for that).
    }

    fn layout(
        &mut self,
        ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        data: &EditorState,
        env: &Env,
    ) -> Size {
        self.icon.layout(ctx, bc, data, env)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, data: &EditorState, env: &Env) {
        self.icon.paint(ctx, data, env);
    }
}
