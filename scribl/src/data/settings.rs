use druid::{Data, Lens};
use scribl_curves::{Effect, Effects, FadeEffect, StrokeStyle, TimeDiff};

use crate::config::Config;

/// How far are they allowed to zoom in?
pub const MAX_ZOOM: f64 = 8.0;

/// This piece of data contains the various settings that affect recording.
///
/// Many of these fields have a button in the UI for changing that setting.
#[derive(Clone, Data, Lens)]
pub struct Settings {
    pub recording_speed: RecordingSpeed,

    /// Zoom level of the drawing pane. A zoom of 1.0 gives the best fit of the drawing into the
    /// drawing pane; we only allow zooming in from there.
    ///
    /// This would be more natural as something private to `DrawingPane`, but the menus need to
    /// access it in order to check when the zoom actions should be enabled/disabled.
    pub zoom: f64,

    /// When true, the "fade out" toggle button is pressed down.
    pub fade_enabled: bool,

    /// The current pen size, as selected in the UI.
    pub pen_size: PenSize,

    /// The current denoise setting, as selected in the UI.
    pub denoise_setting: DenoiseSetting,

    pub palette: crate::widgets::PaletteData,
}

impl Settings {
    pub fn new(config: &Config) -> Settings {
        let denoise_setting = if !config.audio_input.remove_noise {
            DenoiseSetting::DenoiseOff
        } else if config.audio_input.vad_threshold <= 0.0 {
            DenoiseSetting::DenoiseOn
        } else {
            DenoiseSetting::Vad
        };

        Settings {
            denoise_setting,
            recording_speed: RecordingSpeed::Slow,
            zoom: 1.0,
            fade_enabled: false,
            pen_size: PenSize::Small,
            palette: crate::widgets::PaletteData::default(),
        }
    }

    fn selected_effects(&self) -> Effects {
        let mut ret = Effects::default();
        if self.fade_enabled {
            ret.add(Effect::Fade(FadeEffect {
                pause: TimeDiff::from_micros(250_000),
                fade: TimeDiff::from_micros(250_000),
            }));
        }
        ret
    }

    pub fn cur_style(&self) -> StrokeStyle {
        StrokeStyle {
            color: self.palette.selected_color().clone(),
            thickness: self.pen_size.size_fraction(),
            effects: self.selected_effects(),
        }
    }

    pub fn can_zoom_in(&self) -> bool {
        self.zoom < MAX_ZOOM
    }

    pub fn can_zoom_out(&self) -> bool {
        self.zoom > 1.0
    }

    pub fn zoom_in(&mut self) {
        self.zoom = (self.zoom * 1.25).min(MAX_ZOOM);
    }

    pub fn zoom_out(&mut self) {
        self.zoom = (self.zoom / 1.25).max(1.0);
    }

    pub fn zoom_reset(&mut self) {
        self.zoom = 1.0;
    }
}

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum RecordingSpeed {
    Paused,
    Slower,
    Slow,
    Normal,
}

impl RecordingSpeed {
    pub fn factor(&self) -> f64 {
        match self {
            RecordingSpeed::Paused => 0.0,
            RecordingSpeed::Slower => 1.0 / 8.0,
            RecordingSpeed::Slow => 1.0 / 3.0,
            RecordingSpeed::Normal => 1.0,
        }
    }
}

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum PenSize {
    Small,
    Medium,
    Big,
}

impl PenSize {
    /// Returns the diameter of the pen, as a fraction of the width of the drawing.
    pub fn size_fraction(&self) -> f64 {
        match self {
            PenSize::Small => 0.002,
            PenSize::Medium => 0.004,
            PenSize::Big => 0.012,
        }
    }
}

#[derive(Clone, Copy, Data, PartialEq, Eq)]
pub enum DenoiseSetting {
    DenoiseOff,
    DenoiseOn,
    Vad,
}
