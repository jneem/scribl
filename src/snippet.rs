use druid::Color;
use druid::kurbo::{BezPath, PathEl, Point};

use crate::lerp::Lerp;

#[derive(Debug)]
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

impl From<Curve> for LerpedCurve {
    // TODO: this panics if c is empty
    fn from(c: Curve) -> LerpedCurve {
        let start_end = vec![*c.time_us.first().unwrap(), *c.time_us.last().unwrap()];
        let lerp = Lerp::new(start_end.clone(), start_end);
        LerpedCurve {
            curve: c,
            lerp,
        }
    }
}

impl LerpedCurve {
    pub fn path_until(&self, time_us: i64) -> &[PathEl] {
        let local_time = self.lerp.unlerp_clamped(time_us);
        let idx = match self.curve.time_us.binary_search(&local_time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.curve.path.elements()[..idx]
    }
}

#[derive(Default)]
pub struct Snippets {
    // TODO: we'll want maps for accessing curves by start/end indices, etc.
    pub curves: Vec<LerpedCurve>,
}

impl Snippets {
    pub fn insert(&mut self, curve: Curve) {
        dbg!(&curve);
        self.curves.push(curve.into());
    }
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

