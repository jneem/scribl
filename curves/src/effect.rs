//! This module contains visual effects that can be applied to snippets.
//!
//! (Or at least, it does in principle. There's only one effect right now.)

use druid::Data;
use serde::de::{Deserializer, SeqAccess, Visitor};
use serde::ser::{SerializeSeq, Serializer};
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

// TODO: how do we deserialize an "open" enum? We'd like to be able to read files
// with unrecognized effects.
#[derive(Clone, Data, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum Effect {
    Fade(FadeEffect),
}

/// A collection of effects.
#[derive(Clone, Data, Debug, Default, Eq, PartialEq)]
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

// We serialize effects as a sequence, so that we can implement more effects
// without breaking the file format.
impl Serialize for Effects {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        let mut seq = ser.serialize_seq(None)?;

        if let Some(fade) = &self.fade {
            seq.serialize_element(&Effect::Fade(fade.clone()))?;
        }

        seq.end()
    }
}

impl<'de> Deserialize<'de> for Effects {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Effects, D::Error> {
        de.deserialize_seq(EffectsVisitor)
    }
}

struct EffectsVisitor;

impl<'de> Visitor<'de> for EffectsVisitor {
    type Value = Effects;
    fn expecting(&self, fmt: &mut std::fmt::Formatter) -> std::fmt::Result {
        fmt.write_str("a list of effects")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut access: A) -> Result<Effects, A::Error> {
        let mut ret = Effects::default();

        if let Some(effect) = access.next_element()? {
            match effect {
                Effect::Fade(fade) => {
                    ret.fade = Some(fade);
                }
            }
        }

        Ok(ret)
    }
}
