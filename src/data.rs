use druid::kurbo::PathEl;
use druid::{Color, Data, Lens, Point};
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::ops::Deref;
use std::sync::Arc;

use crate::lerp::Lerp;
use crate::snippet::{Curve, SnippetId};
use crate::widgets::ToggleButtonState;

#[derive(Clone, Data)]
pub struct CurveInProgressData {
    #[druid(ignore)]
    inner: Arc<RefCell<Curve>>,

    // Data comparison is done using only the curve's length, since the length grows with
    // every modification.
    len: usize,
}

impl CurveInProgressData {
    pub fn new(color: &Color, thickness: f64) -> CurveInProgressData {
        CurveInProgressData {
            inner: Arc::new(RefCell::new(Curve::new(color, thickness))),
            len: 0,
        }
    }

    pub fn move_to(&mut self, p: Point, time: i64) {
        self.inner.borrow_mut().move_to(p, time);
        self.len += 1;
    }

    pub fn line_to(&mut self, p: Point, time: i64) {
        self.inner.borrow_mut().line_to(p, time);
        self.len += 1;
    }

    pub fn into_curve(self) -> Curve {
        self.inner.replace(Curve::new(&Color::rgb8(0, 0, 0), 1.0))
    }
}

#[derive(Data, Debug, Clone)]
pub struct SnippetData {
    pub curve: Arc<Curve>,
    pub lerp: Arc<Lerp>,

    /// Controls whether the snippet ever ends. If `None`, it means that the snippet will remain
    /// forever; if `Some(t)` it means that the snippet will disappear at time `t`.
    pub end: Option<i64>,
}

#[derive(Clone, Data, Default)]
pub struct SnippetsData {
    last_id: u64,
    snippets: Arc<BTreeMap<SnippetId, SnippetData>>,
}

impl SnippetData {
    // TODO: this panics if the curve is empty
    pub fn new(curve: Curve) -> SnippetData {
        let start_end = vec![
            *curve.time_us.first().unwrap(),
            *curve.time_us.last().unwrap(),
        ];
        let lerp = Lerp::new(start_end.clone(), start_end);
        SnippetData {
            curve: Arc::new(curve),
            lerp: Arc::new(lerp),
            end: None,
        }
    }

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

impl SnippetsData {
    pub fn with_new_snippet(&self, curve: Curve) -> SnippetsData {
        let mut ret = self.clone();
        ret.last_id += 1;
        let id = SnippetId(ret.last_id);
        let new_snippet = SnippetData::new(curve);
        let mut map = ret.snippets.deref().clone();
        map.insert(id, new_snippet);
        ret.snippets = Arc::new(map);
        ret
    }

    pub fn with_replacement_snippet(&self, id: SnippetId, new: SnippetData) -> SnippetsData {
        assert!(id.0 <= self.last_id);
        let mut ret = self.clone();
        let mut map = ret.snippets.deref().clone();
        map.insert(id, new);
        ret.snippets = Arc::new(map);
        ret
    }

    pub fn with_new_lerp(&self, id: SnippetId, lerp_from: i64, lerp_to: i64) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.lerp = Arc::new(snip.lerp.with_new_lerp(lerp_from, lerp_to));
        self.with_replacement_snippet(id, snip)
    }

    pub fn with_truncated_snippet(&self, id: SnippetId, time: i64) -> SnippetsData {
        let mut snip = self.snippet(id).clone();
        snip.end = Some(time);
        self.with_replacement_snippet(id, snip)
    }

    pub fn snippet(&self, id: SnippetId) -> &SnippetData {
        self.snippets.get(&id).unwrap()
    }

    pub fn snippets(&self) -> impl Iterator<Item = (SnippetId, &SnippetData)> {
        self.snippets.iter().map(|(k, v)| (*k, v))
    }

    pub fn layout_non_overlapping(&self, num_slots: usize) -> Option<HashMap<SnippetId, usize>> {
        let mut bounds: Vec<_> = self.snippets().map(SnippetBounds::new).collect();
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
    fn new(data: (SnippetId, &SnippetData)) -> SnippetBounds {
        SnippetBounds {
            start_us: data.1.lerp.first(),
            end_us: data.1.end,
            id: data.0,
        }
    }
}

/// This data contains the entire state of the app.
#[derive(Clone, Data, Lens)]
pub struct ScribbleState {
    pub new_snippet: Option<CurveInProgressData>,
    pub snippets: SnippetsData,
    pub selected_snippet: Option<SnippetId>,
    pub action: CurrentAction,

    pub time_us: i64,
    pub mark: Option<i64>,

    // This is a bit of an odd one out, since it's specifically for input handling in the
    // drawing-pane widget. If there get to be more of these, maybe they should get split out.
    pub mouse_down: bool,

    pub line_thickness: f64,
    pub line_color: Color,
}

impl Default for ScribbleState {
    fn default() -> ScribbleState {
        ScribbleState {
            new_snippet: None,
            snippets: SnippetsData::default(),
            selected_snippet: None,
            action: CurrentAction::Idle,
            time_us: 0,
            mark: None,
            mouse_down: false,
            line_thickness: 5.0,
            line_color: Color::rgb8(0, 255, 0),
        }
    }
}

impl ScribbleState {
    pub fn curve_in_progress<'a>(&'a self) -> Option<impl std::ops::Deref<Target = Curve> + 'a> {
        self.new_snippet.as_ref().map(|s| s.inner.borrow())
    }

    pub fn start_recording(&mut self) {
        assert!(self.new_snippet.is_none());
        assert_eq!(self.action, CurrentAction::Idle);
        dbg!(self.time_us);
        self.new_snippet = Some(CurveInProgressData::new(
            &self.line_color,
            self.line_thickness,
        ));
        self.action = CurrentAction::WaitingToRecord;
    }

    pub fn stop_recording(&mut self) {
        assert!(
            self.action == CurrentAction::Recording
                || self.action == CurrentAction::WaitingToRecord
        );
        let new_snippet = self
            .new_snippet
            .take()
            .expect("Tried to stop recording, but we hadn't started!");
        self.action = CurrentAction::Idle;
        let new_curve = new_snippet.into_curve();
        if !new_curve.path.elements().is_empty() {
            self.snippets = self.snippets.with_new_snippet(new_curve);
            self.selected_snippet = Some(SnippetId(self.snippets.last_id));
        }
    }

    pub fn start_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Idle);
        self.action = CurrentAction::Playing;
        self.time_us = 0;
    }

    pub fn stop_playing(&mut self) {
        assert_eq!(self.action, CurrentAction::Playing);
        self.action = CurrentAction::Idle;
    }
}

#[derive(Clone, Copy, Data, Debug, PartialEq)]
pub enum CurrentAction {
    WaitingToRecord,
    Recording,
    Playing,

    /// Fast-forward or reverse. The parameter is the speed factor, negative for reverse.
    Scanning(f64),
    Idle,
}

impl Default for CurrentAction {
    fn default() -> CurrentAction {
        CurrentAction::Idle
    }
}

impl CurrentAction {
    pub fn rec_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            WaitingToRecord => ToggledOn,
            Recording => ToggledOn,
            Idle => ToggledOff,
            Playing => Disabled,
            Scanning(_) => Disabled,
        }
    }

    pub fn play_toggle(&self) -> ToggleButtonState {
        use CurrentAction::*;
        use ToggleButtonState::*;
        match *self {
            WaitingToRecord => Disabled,
            Recording => Disabled,
            Scanning(_) => Disabled,
            Playing => ToggledOn,
            Idle => ToggledOff,
        }
    }

    pub fn is_idle(&self) -> bool {
        *self == CurrentAction::Idle
    }

    pub fn is_recording(&self) -> bool {
        *self == CurrentAction::Recording
    }

    pub fn is_waiting_to_record(&self) -> bool {
        *self == CurrentAction::WaitingToRecord
    }

    pub fn is_ticking(&self) -> bool {
        *self == CurrentAction::Recording || *self == CurrentAction::Playing
    }

    pub fn is_scanning(&self) -> bool {
        if let CurrentAction::Scanning(_) = *self {
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Creates a snippet that is empty, but has a starting and (possibly) an ending time.
    fn snip(begin: i64, end: Option<i64>) -> SnippetData {
        SnippetData {
            curve: Arc::new(Curve::new(&Color::rgb8(0, 0, 0), 1.0)),
            lerp: Arc::new(Lerp::new(vec![0], vec![begin])),
            end,
        }
    }

    macro_rules! snips {
        ( $(($begin:expr, $end:expr)),* ) => {
            {
                let mut ret = SnippetsData::default();
                let mut map = BTreeMap::new();
                $(
                    ret.last_id += 1;
                    map.insert(SnippetId(ret.last_id), snip($begin, $end));
                )*
                ret.snippets = Arc::new(map);
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
