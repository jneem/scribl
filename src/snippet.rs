use druid::kurbo::{BezPath, PathEl, Point};
use druid::{Color, Data};
use std::collections::{BTreeMap, HashMap};

use crate::lerp::Lerp;

#[derive(Clone, Data, Debug)]
pub struct Curve {
    pub path: BezPath,
    #[druid(same_fn = "PartialEq::eq")]
    pub time_us: Vec<i64>,
    pub color: Color,
    pub thickness: f64,
}

/// A curve under construction. (TODO: can we delete this?)
pub struct CurveInProgress {
    pub curve: Curve,
}

#[derive(Data, Clone)]
pub struct LerpedCurve {
    pub curve: Curve,
    pub lerp: Lerp,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub end: Option<i64>,
}

impl CurveInProgress {
    pub fn new(color: &Color, thickness: f64) -> CurveInProgress {
        CurveInProgress {
            curve: Curve::new(color, thickness),
        }
    }

    pub fn move_to(&mut self, p: Point, time: i64) {
        self.curve.move_to(p, time);
    }

    pub fn line_to(&mut self, p: Point, time: i64) {
        self.curve.line_to(p, time);
    }
}

impl From<Curve> for LerpedCurve {
    // TODO: this panics if c is empty
    fn from(c: Curve) -> LerpedCurve {
        let start_end = vec![*c.time_us.first().unwrap(), *c.time_us.last().unwrap()];
        let lerp = Lerp::new(start_end.clone(), start_end);
        LerpedCurve {
            curve: c,
            lerp,
            end: None,
        }
    }
}

impl LerpedCurve {
    pub fn path_at(&self, time_us: i64) -> &[PathEl] {
        if let Some(end) = self.end {
            if time_us > end {
                return &[];
            }
        }

        let local_time = self.lerp.unlerp_clamped(time_us);
        let idx = match self.curve.time_us.binary_search(&local_time) {
            Ok(i) => i + 1,
            Err(i) => i,
        };
        &self.curve.path.elements()[..idx]
    }

    pub fn start_time(&self) -> i64 {
        self.lerp.first()
    }

    /// The last time at which the snippet changed.
    pub fn last_draw_time(&self) -> i64 {
        self.lerp.last()
    }

    /// The time at which this snippet should disappear.
    pub fn end_time(&self) -> Option<i64> {
        self.end
    }
}

/// Snippets are identified by unique ids.
#[derive(Clone, Copy, Data, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
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

    pub fn curves(&self) -> impl Iterator<Item = &LerpedCurve> {
        self.curves.values()
    }

    pub fn iter(&self) -> impl Iterator<Item = (SnippetId, &LerpedCurve)> {
        self.curves.iter().map(|(a, b)| (*a, b))
    }

    pub fn last_time(&self) -> i64 {
        self.curves().map(|c| c.lerp.last()).max().unwrap_or(0)
    }

    pub fn layout_non_overlapping(&self, num_slots: usize) -> Option<HashMap<SnippetId, usize>> {
        let mut bounds: Vec<_> = self.iter().map(SnippetBounds::new).collect();
        bounds.sort_by_key(|b| b.start_us);

        let mut row_ends = vec![Some(0i64); num_slots as usize];
        let mut ret = HashMap::new();
        'bounds: for b in &bounds {
            for (row_idx, end) in row_ends.iter_mut().enumerate() {
                if let Some(finite_end_time) = *end {
                    if finite_end_time == 0 || b.start_us > finite_end_time {
                        *end = b.end_us;
                        ret.insert(b.id, row_idx);
                        continue 'bounds;
                    }
                }
            }
            return None;
        }
        Some(ret)
    }
}

struct SnippetBounds {
    start_us: i64,
    end_us: Option<i64>,
    id: SnippetId,
}

impl SnippetBounds {
    fn new(data: (SnippetId, &LerpedCurve)) -> SnippetBounds {
        SnippetBounds {
            start_us: data.1.lerp.first(),
            end_us: data.1.end,
            id: data.0,
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    // Creates a LerpedCurve that is empty, but has a starting and (possibly) an ending time.
    fn snip(begin: i64, end: Option<i64>) -> LerpedCurve {
        LerpedCurve {
            curve: Curve::new(&Color::rgb8(0, 0, 0), 1.0),
            lerp: Lerp::new(vec![0], vec![begin]),
            end,
        }
    }

    macro_rules! snips {
        ( $(($begin:expr, $end:expr)),* ) => {
            {
                let mut ret = Snippets::default();
                $(
                    ret.last_id += 1;
                    ret.curves.insert(SnippetId(ret.last_id), snip($begin, $end));
                )*
                ret
            }
        }
    }

    #[test]
    fn layout_infinite() {
        let snips = snips!((0, None), (1, None));
        let layout = snips.layout_non_overlapping(3).unwrap();
        dbg!(&layout);
        assert_eq!(layout[&SnippetId(1)], 0);
        assert_eq!(layout[&SnippetId(2)], 1);
    }
}
