//! This module contains visual effects that can be applied to snippets.
//!
//! (Or at least, it does in principle. There's only one effect right now.)

use druid::Data;
use serde::{Deserialize, Serialize};

use crate::time::Diff;

/// A fade effect.
///
/// When a segment is finished, it will start fading out.
#[derive(Clone, Data, Debug, Eq, Serialize, Deserialize, PartialEq)]
pub struct FadeEffect {
    /// After the segment finishes, it will remain at full opacity for this duration.
    /// Then it will start fading out.
    pub pause: Diff,

    /// The segment will fade out (linearly interpolated) for this length of time.
    pub fade: Diff,
}

#[derive(Clone, Data, Debug, Eq, Serialize, Deserialize, PartialEq)]
pub enum Effect {
    Fade(FadeEffect),
}

/// A collection of effects.
#[derive(Clone, Data, Debug, Default, Eq, Serialize, Deserialize, PartialEq)]
pub struct Effects {
    fade: Option<FadeEffect>,
}

impl Effects {
    pub fn add(&mut self, effect: Effect) {
        match effect {
            Effect::Fade(fade) => self.fade = Some(fade),
        }
    }

    pub fn fade(&self) -> Option<&FadeEffect> {
        self.fade.as_ref()
    }
}
