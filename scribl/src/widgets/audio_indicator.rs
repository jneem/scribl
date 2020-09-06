use druid::widget::prelude::*;
use druid::{Color, Rect};

use crate::cmd;
use crate::editor_state::{DenoiseSetting, EditorState};

static BAR_COLORS: &[Color] = &[
    Color::rgb8(87, 157, 66),
    Color::rgb8(87, 157, 66),
    Color::rgb8(166, 205, 87),
    Color::rgb8(166, 205, 87),
    Color::rgb8(255, 214, 0),
    Color::rgb8(248, 151, 31),
];

pub struct AudioIndicator {
    height: f64,
    // The number of color bands to display
    loudness_bands: usize,
}

fn calc_bands(loudness: f32) -> usize {
    // 0.0 or above means all the bands. Then we do 1 band per 10 dB.
    BAR_COLORS
        .len()
        .saturating_sub(((-loudness).max(0.0) / 10.0).floor() as usize)
}

impl AudioIndicator {
    pub fn new(height: f64) -> AudioIndicator {
        AudioIndicator {
            height,
            loudness_bands: 0,
        }
    }
}

// TODO: can we make a narrower state?
impl Widget<EditorState> for AudioIndicator {
    fn event(&mut self, ctx: &mut EventCtx, ev: &Event, data: &mut EditorState, _env: &Env) {
        if let Event::Command(c) = ev {
            if let Some(status) = c.get(cmd::RECORDING_AUDIO_STATUS) {
                let vad = data.denoise_setting != DenoiseSetting::Vad
                    || status.vad >= data.config.audio_input.vad_threshold;
                let loudness_bands = if vad { calc_bands(status.loudness) } else { 0 };

                if loudness_bands != self.loudness_bands {
                    self.loudness_bands = loudness_bands;
                    ctx.request_paint();
                }
            }
        }
    }

    fn lifecycle(
        &mut self,
        _ctx: &mut LifeCycleCtx,
        _ev: &LifeCycle,
        _data: &EditorState,
        _env: &Env,
    ) {
    }

    fn update(
        &mut self,
        _ctx: &mut UpdateCtx,
        _old_data: &EditorState,
        _data: &EditorState,
        _env: &Env,
    ) {
    }

    fn layout(
        &mut self,
        _ctx: &mut LayoutCtx,
        bc: &BoxConstraints,
        _data: &EditorState,
        _env: &Env,
    ) -> Size {
        let size = Size::new(10.0, self.height);
        bc.constrain(size)
    }

    fn paint(&mut self, ctx: &mut PaintCtx, _data: &EditorState, _env: &Env) {
        let size = ctx.size();
        let band_height = size.height / (BAR_COLORS.len() as f64 * 1.5);
        let band_offset = band_height * 1.5;

        for i in 0..self.loudness_bands {
            let bottom = size.height - i as f64 * band_offset;
            let rect = Rect::new(0.0, bottom - band_height, size.width, bottom);
            ctx.fill(rect, &BAR_COLORS[i]);
        }
    }
}
