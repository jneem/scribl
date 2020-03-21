use druid::kurbo::{BezPath, PathEl, Point};
use druid::{Color, Data};
use std::collections::BTreeMap;
use std::convert::TryInto;
use std::time::Instant;

use crate::lerp::Lerp;

#[derive(Clone, Data, Debug)]
pub struct Curve {
    pub path: BezPath,
    #[druid(same_fn = "PartialEq::eq")]
    pub time_us: Vec<i64>,
    pub color: Color,
    pub thickness: f64,
}

/// A curve under construction.
pub struct CurveInProgress {
    pub curve: Curve,
    pub logical_start_time_us: i64,
    pub wall_start_time: Instant,
}

#[derive(Data, Clone)]
pub struct LerpedCurve {
    pub curve: Curve,
    pub lerp: Lerp,
}

impl CurveInProgress {
    pub fn new(time_us: i64, color: &Color, thickness: f64) -> CurveInProgress {
        CurveInProgress {
            curve: Curve::new(color, thickness),
            logical_start_time_us: time_us,
            wall_start_time: Instant::now(),
        }
    }

    fn elapsed_us(&self) -> i64 {
        let elapsed: i64 = Instant::now()
            .duration_since(self.wall_start_time)
            .as_micros()
            .try_into()
            .expect("this has been running too long!");
        elapsed + self.logical_start_time_us
    }

    pub fn move_to(&mut self, p: Point) {
        self.curve.move_to(p, self.elapsed_us());
    }

    pub fn line_to(&mut self, p: Point) {
        self.curve.line_to(p, self.elapsed_us());
    }
}

impl From<Curve> for LerpedCurve {
    // TODO: this panics if c is empty
    fn from(c: Curve) -> LerpedCurve {
        let start_end = vec![*c.time_us.first().unwrap(), *c.time_us.last().unwrap()];
        let lerp = Lerp::new(start_end.clone(), start_end);
        LerpedCurve { curve: c, lerp }
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

/// Snippets are identified by unique ids.
#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
// TODO: remove the pub
pub struct SnippetId(pub u64);

#[derive(Clone, Default)]
pub struct Snippets {
    // TODO: we'll want maps for accessing curves by start/end indices, etc.
    pub curves: BTreeMap<SnippetId, LerpedCurve>,
    pub last_id: u64,
}

impl Snippets {
    pub fn insert(&mut self, curve: Curve) -> SnippetId {
        dbg!(&curve);
        self.last_id += 1;
        let id = SnippetId(self.last_id);

        self.curves.insert(id, curve.into());
        id
    }

    pub fn curves(&self) -> impl Iterator<Item=&LerpedCurve> {
        self.curves.values()
    }

    pub fn iter(&self) -> impl Iterator<Item=(SnippetId, &LerpedCurve)> {
        self.curves.iter().map(|(a, b)| (*a, b))
    }

    pub fn last_time(&self) -> i64 {
        self.curves().map(|c| c.lerp.last()).max().unwrap_or(0)
    }
}

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
