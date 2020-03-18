use druid::Color;
use druid::kurbo::{BezPath, Point};

use crate::lerp::Lerp;

pub struct Curve {
    pub path: BezPath,
    pub time_us: Vec<i64>,
    pub color: Color,
    pub thickness: f64,
}

pub struct LerpedCurve {
    pub curve: Curve,
    pub lerp: Lerp,
}

#[derive(Default)]
pub struct Snippets {
    // TODO: we'll want maps for accessing curves by start/end indices, etc.
    pub curves: Vec<LerpedCurve>,
}

impl Curve {
    pub fn new() -> Curve {
        Curve {
            path: BezPath::new(),
            time_us: Vec::new(),
            color: Color::rgb8(0, 255, 0),
            thickness: 1.0,
        }
    }
}

impl Curve {
    pub fn line_to(&mut self, p: Point, time_us: i64) {
        self.path.line_to(p);
        self.time_us.push(time_us);
    }

    pub fn move_to(&mut self, p: Point, time_us: i64) {
        self.path.move_to(p);
        self.time_us.push(time_us);
    }
}

impl Default for Curve {
    fn default() -> Curve {
        Curve::new()
    }
}

