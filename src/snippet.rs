use druid::kurbo::{BezPath, Point};
use druid::{Color, Data};
use serde::{Deserialize, Serialize};

//#[derive(Deserialize, Serialize)]
#[derive(Clone, Debug)]
pub struct Curve {
    pub path: BezPath,
    pub time_us: Vec<i64>,
    pub color: Color,
    pub thickness: f64,
}

/// Snippets are identified by unique ids.
#[derive(Deserialize, Serialize)]
#[derive(Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
// TODO: remove the pub
pub struct SnippetId(pub u64);

impl Curve {
    pub fn new(color: &Color, thickness: f64) -> Curve {
        Curve {
            path: BezPath::new(),
            time_us: Vec::new(),
            color: color.clone(),
            thickness,
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
