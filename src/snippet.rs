use druid::kurbo::{BezPath, Point};
use druid::{Color, Data};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::time::Time;

mod serde_path {
    use super::*;

    pub fn serialize<S: Serializer>(path: &BezPath, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_str(&path.to_svg())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<BezPath, D::Error> {
        let s = String::deserialize(de)?;
        // TODO: once serde support appears in kurbo, drop this
        Ok(BezPath::from_svg(&s).unwrap())
    }
}

mod serde_color {
    use super::*;

    pub fn serialize<S: Serializer>(c: &Color, ser: S) -> Result<S::Ok, S::Error> {
        ser.serialize_u32(c.as_rgba_u32())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(de: D) -> Result<Color, D::Error> {
        Ok(Color::from_rgba32_u32(u32::deserialize(de)?))
    }
}

#[derive(Deserialize, Serialize, Clone, Debug)]
pub struct Curve {
    #[serde(with = "serde_path")]
    pub path: BezPath,
    pub times: Vec<Time>,
    #[serde(with = "serde_color")]
    pub color: Color,
    pub thickness: f64,
}

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize, Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
// TODO: remove the pub
pub struct SnippetId(pub u64);

impl Curve {
    pub fn new(color: &Color, thickness: f64) -> Curve {
        Curve {
            path: BezPath::new(),
            times: Vec::new(),
            color: color.clone(),
            thickness,
        }
    }
}

impl Curve {
    pub fn line_to(&mut self, p: Point, time: Time) {
        self.path.line_to(p);
        self.times.push(time);
    }

    pub fn move_to(&mut self, p: Point, time: Time) {
        self.path.move_to(p);
        self.times.push(time);
    }
}
