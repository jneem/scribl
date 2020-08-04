//! This module is in charge of layout out the snippets in the timeline.
//!
//! The rough idea is: we lay out the snippets from top to bottom. At each step, we "drape" the new
//! snippet around the "contours" of the existing snippets. More precisely, we use a skyline data
//! structure to represent the existing snippets.

use druid::kurbo::{BezPath, Point, Rect, Vec2};
use std::collections::HashMap;
use std::hash::Hash;

use scribl_curves::{SnippetData, SnippetId, Time};

use crate::audio::{AudioSnippetData, AudioSnippetId};

/// An element of the skyline. We don't store the beginning -- it's the end of the previous
/// building.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Building {
    y: f64,
    end_x: f64,
}

/// A bare-bones skyline implementation. We don't store the starting x coord -- for the skyline that
/// represents the whole timeline, we always start at zero. For the skyline representing just the
/// new snippet that we're adding, we just keep track of the start separately.
#[derive(Clone, Debug, Default, PartialEq)]
struct Skyline {
    buildings: Vec<Building>,
}

/// A collection of rectangles describing the layout of a single snippet in the timeline. These
/// rectangles will be increasing in the `x` coordinate, and usually overlapping a bit (depending
/// on the `overlap` parameter in `Parameters`). For example, they might look like
///
/// ```
///                                         +--------------------+
///                     +----------------------+                 |
/// +---------------------+                 |  |                 |
/// |                   | |                 +--------------------+
/// |                   | |                    |
/// |                   +----------------------+
/// |                     |
/// +---------------------+
/// ```
#[derive(Clone, Debug)]
pub struct SnippetShape {
    pub rects: Vec<Rect>,
}

/// This is an intermediate data-type that we use when converting a `SnippetShape` into a
/// nice-looking curved path. Basically, we replace each `Rect` with a collection of the
/// four "important" points after taking overlapping neighbors into account. In the example
/// below, the important points in the middle rect are marked with `o`:
///
/// ```
///                                         +--------------------+
///                     o-------------------o--+                 |
/// +---------------------+                 |  |                 |
/// |                   | |                 +--------------------+
/// |                   | |                    |
/// |                   +-o--------------------o
/// |                     |
/// +---------------------+
/// ```
#[derive(Debug)]
struct Quad {
    top_left: Point,
    bottom_left: Point,
    top_right: Point,
    bottom_right: Point,
}

impl Quad {
    fn new(rect: &Rect) -> Quad {
        Quad {
            top_left: (rect.x0, rect.y0).into(),
            bottom_left: (rect.x0, rect.y1).into(),
            top_right: (rect.x1, rect.y0).into(),
            bottom_right: (rect.x1, rect.y1).into(),
        }
    }

    fn bottom_center(&self) -> Point {
        self.bottom_left.midpoint(self.bottom_right)
    }

    fn bottom_width(&self) -> f64 {
        self.bottom_right.x - self.bottom_left.x
    }

    fn top_center(&self) -> Point {
        self.top_left.midpoint(self.top_right)
    }

    fn top_width(&self) -> f64 {
        self.top_right.x - self.top_left.x
    }
}

impl SnippetShape {
    /// See the doc comment to `Quads` for an example of what this is doing.
    fn to_quads(&self) -> Vec<Quad> {
        let mut ret: Vec<_> = self.rects.iter().map(Quad::new).collect();

        if ret.is_empty() {
            return ret;
        }

        for i in 1..ret.len() {
            if ret[i - 1].bottom_right.y > ret[i].bottom_left.y {
                ret[i].bottom_left.x = ret[i - 1].bottom_right.x;
            } else {
                ret[i - 1].bottom_right.x = ret[i].bottom_left.x;
            }

            if ret[i - 1].top_right.y > ret[i].top_left.y {
                ret[i - 1].top_right.x = ret[i].top_left.x;
            } else {
                ret[i].top_left.x = ret[i - 1].top_right.x;
            }
        }

        ret
    }

    /// Converts this snippet into a nice-looking path with rounded corners.
    pub fn to_path(&self, radius: f64) -> BezPath {
        let mut ret = BezPath::new();
        if self.rects.is_empty() {
            return ret;
        }

        let quads = self.to_quads();
        let dx = Vec2::new(radius, 0.0);

        let first_pt = if quads[0].bottom_width() >= 2.0 * radius {
            quads[0].bottom_left + dx
        } else {
            quads[0].bottom_center()
        };
        ret.move_to(first_pt);

        // Left-to-right across the bottom.
        let mut prev_ctrl = first_pt;
        for qs in quads.windows(2) {
            let q = &qs[0];
            let next = &qs[1];

            if q.bottom_width() >= 2.0 * radius {
                prev_ctrl = q.bottom_right - dx;
                ret.line_to(prev_ctrl);
            }
            let next_ctrl = if next.bottom_width() >= 2.0 * radius {
                next.bottom_left + dx
            } else {
                next.bottom_center()
            };

            ret.curve_to(prev_ctrl + dx, next_ctrl - dx, next_ctrl);
            prev_ctrl = next_ctrl;
        }

        let q = quads.last().unwrap();
        let mut next_ctrl = q.top_center();
        if q.bottom_width() >= 2.0 * radius {
            prev_ctrl = q.bottom_right - dx;
            next_ctrl = q.top_right - dx;
            ret.line_to(prev_ctrl);
        }
        ret.curve_to(prev_ctrl + dx, next_ctrl + dx, next_ctrl);
        prev_ctrl = next_ctrl;

        // Now backwards across the top
        for qs in quads.windows(2).rev() {
            let q = &qs[1];
            let next = &qs[0];

            if q.top_width() >= 2.0 * radius {
                prev_ctrl = q.top_left + dx;
                ret.line_to(prev_ctrl);
            }
            let next_ctrl = if next.top_width() >= 2.0 * radius {
                next.top_right - dx
            } else {
                next.top_center()
            };

            ret.curve_to(prev_ctrl - dx, next_ctrl + dx, next_ctrl);
            prev_ctrl = next_ctrl;
        }

        if quads[0].top_width() >= 2.0 * radius {
            prev_ctrl = quads[0].top_left + dx;
            ret.line_to(prev_ctrl);
        }

        ret.curve_to(prev_ctrl - dx, first_pt - dx, first_pt);
        ret.close_path();
        ret
    }

    /// Reflects this shape vertically, so that `0.0` is mapped to `bottom`, `1.0` is mapped to
    /// `bottom - 1.0`, etc.
    pub fn reflect_y(&mut self, bottom: f64) {
        for r in &mut self.rects {
            let y1 = bottom - r.y0;
            let y0 = bottom - r.y1;
            r.y0 = y0;
            r.y1 = y1;
        }
    }
}

impl Skyline {
    fn new(end_x: f64) -> Skyline {
        Skyline {
            buildings: vec![Building { y: 0.0, end_x }],
        }
    }

    /// Delete zero-width buildings, and merge adjacent buildings that have the same height.
    fn delete_empty(&mut self) {
        let mut start_x = 0.0;
        self.buildings.retain(|b| {
            if b.end_x <= start_x {
                false
            } else {
                start_x = b.end_x;
                true
            }
        });

        let mut next_y = f64::INFINITY;
        for b in self.buildings.iter_mut().rev() {
            // There doesn't seem to be a "retain" that goes backwards, so we mark buildings as
            // to-be-deleted by setting end_x to zero.
            if b.y == next_y {
                b.end_x = 0.0;
            }
            next_y = b.y;
        }
        self.buildings.retain(|b| b.end_x > 0.0);
    }

    /// Expand all the buildings in the skyline horizontally (both left and right) by the given
    /// amount. The beginning of the first building is unchanged (because we don't store it), and
    /// the end of the last building is also unchanged.
    fn expand_horizontally(&mut self, padding: f64) {
        for i in 1..self.buildings.len() {
            if self.buildings[i - 1].y > self.buildings[i].y {
                self.buildings[i - 1].end_x += padding;
            } else {
                self.buildings[i - 1].end_x -= padding;
            }
        }
        self.delete_empty();
    }

    /// Expand the skyline to ensure that every building has a minimal width.
    fn fill_gaps(&mut self, min_width: f64) {
        let mut start_x = 0.0;
        let mut prev_nonempty: Option<usize> = None;

        // For every building that's too short, we have a choice:
        // - extend it,
        // - cover it with its left neighbor,
        // - cover it with its right neigbor.
        for i in 0..self.buildings.len() {
            if self.buildings[i].end_x - start_x < min_width {
                let cover_left = prev_nonempty
                    .map(|j| self.buildings[j].y > self.buildings[i].y)
                    .unwrap_or(false);
                let cover_right =
                    i + 1 < self.buildings.len() && self.buildings[i + 1].y > self.buildings[i].y;

                match (cover_left, cover_right) {
                    (false, false) => {
                        self.buildings[i].end_x = start_x + min_width;
                        prev_nonempty = Some(i);
                    }
                    (true, false) => {
                        self.buildings[prev_nonempty.unwrap()].end_x = self.buildings[i].end_x
                    }
                    (false, true) => self.buildings[i].end_x = start_x,
                    (true, true) => {
                        let prev = prev_nonempty.unwrap();
                        if self.buildings[prev].y <= self.buildings[i + 1].y {
                            self.buildings[i].end_x = start_x;
                        } else {
                            self.buildings[prev].end_x = self.buildings[i].end_x;
                        }
                    }
                }
            } else {
                prev_nonempty = Some(i);
            }
            start_x = self.buildings[i].end_x;
        }

        self.delete_empty();
    }

    fn add_rect(
        &self,
        start_x: f64,
        end_x: f64,
        height: f64,
        min_width: f64,
        new_part: &mut Skyline,
    ) {
        // Find the first building that ends strictly after `start_x`.
        let start_idx = match self
            .buildings
            .binary_search_by(|b| b.end_x.partial_cmp(&start_x).unwrap())
        {
            Ok(idx) => idx + 1,
            Err(idx) => idx,
        };

        assert!(start_idx == self.buildings.len() || self.buildings[start_idx].end_x > start_x);

        let mut idx = start_idx;
        let mut x = start_x;
        while idx < self.buildings.len() {
            let min_end = x + min_width;
            let orig_idx = idx;
            let mut y0 = self.buildings[idx].y;
            while idx + 1 < self.buildings.len() && min_end >= self.buildings[idx].end_x {
                idx += 1;
                y0 = y0.max(self.buildings[idx].y);
            }
            let this_end_x = if orig_idx < idx {
                min_end
            } else {
                self.buildings[idx].end_x.min(end_x).max(min_end)
            };
            new_part.buildings.push(Building {
                y: y0 + height,
                end_x: this_end_x,
            });
            x = this_end_x;

            if end_x <= x {
                break;
            }
            if this_end_x >= self.buildings[idx].end_x {
                idx += 1;
            }
        }
    }

    fn update_skyline(&mut self, start_x: f64, other: &[Building]) {
        let mut new = Vec::new();
        let mut merged = false;
        let mut i = 0;
        while i < self.buildings.len() {
            if !merged && start_x < self.buildings[i].end_x {
                new.push(Building {
                    end_x: start_x,
                    y: self.buildings[i].y,
                });
                new.extend_from_slice(other);
                let x = other.last().map(|b| b.end_x).unwrap_or(0.0);
                while i < self.buildings.len() && self.buildings[i].end_x <= x {
                    i += 1;
                }

                merged = true;
                if i == self.buildings.len() {
                    break;
                }
            }

            new.push(self.buildings[i]);
            i += 1;
        }

        self.buildings = new;
    }

    fn to_rects(&self, mut start_x: f64, thick_count: usize, params: &Parameters) -> Vec<Rect> {
        let mut ret = Vec::new();
        for w in self.buildings.windows(2) {
            let end_x = if w[1].y > w[0].y {
                w[0].end_x
            } else {
                w[0].end_x + params.overlap
            };
            let height = if ret.len() < thick_count {
                params.thick_height
            } else {
                params.thin_height
            };
            let y1 = w[0].y;
            let y0 = y1 - height;

            ret.push(Rect {
                x0: start_x,
                x1: end_x,
                y0,
                y1,
            });

            start_x = if w[1].y > w[0].y {
                w[0].end_x - params.overlap
            } else {
                w[0].end_x
            };
        }

        if let Some(last) = self.buildings.last() {
            let height = if ret.len() < thick_count {
                params.thick_height
            } else {
                params.thin_height
            };
            let y1 = last.y;
            let y0 = y1 - height;
            ret.push(Rect {
                x0: start_x,
                x1: last.end_x,
                y0,
                y1,
            });
        }
        ret
    }

    fn add_snippet<Id>(&mut self, b: &SnippetBounds<Id>, params: &Parameters) -> SnippetShape {
        let mut snip = Skyline {
            buildings: Vec::new(),
        };
        let p = |x: Time| x.as_micros() as f64 * params.pixels_per_usec;
        let thick_end = b.thin.or(b.end).map(p).unwrap_or(params.end_x);

        self.add_rect(
            p(b.start),
            thick_end,
            params.thick_height + params.v_padding,
            params.min_width,
            &mut snip,
        );

        // Keep track of the number of thick segments, so that later we know which parts of `snip`
        // are thin, and which parts are thick.
        // TODO: maybe better for add_rect to produce Rects and then we convert to skyline later?
        let thick_count = snip.buildings.len();

        if let Some(thin) = b.thin {
            let thin_end = b.end.map(p).unwrap_or(params.end_x);
            let thin_start = snip.buildings.last().map(|b| b.end_x).unwrap_or(p(thin));
            self.add_rect(
                thin_start,
                thin_end,
                params.thin_height + params.v_padding,
                params.min_width,
                &mut snip,
            );
        }

        let rects = snip.to_rects(p(b.start), thick_count, params);
        snip.expand_horizontally(params.h_padding + params.overlap);
        if let Some(last) = snip.buildings.last_mut() {
            last.end_x = (last.end_x + params.h_padding).min(params.end_x);
        }

        self.update_skyline(p(b.start) - params.h_padding, &snip.buildings[..]);
        self.fill_gaps(params.min_width);

        SnippetShape { rects }
    }
}

/// A collection of parameters describing how to turn a bunch of snippets into a
/// hopefully-visually-pleasing layout.
pub struct Parameters {
    /// Snippets have thick parts and thin parts (the thick part is the time interval where the
    /// drawing is happening; the thin part then lasts until the snippet disappears). This is
    /// the thickness of the thick part.
    pub thick_height: f64,
    /// The thickness of the thin part.
    pub thin_height: f64,
    /// Horizontal padding that we add between snippets.
    pub h_padding: f64,
    /// Vertical padding that we add between snippets.
    pub v_padding: f64,
    /// The number of pixels per microsecond of timeline time.
    pub pixels_per_usec: f64,
    /// The minimum width of a rectangle in the timeline.
    pub min_width: f64,
    /// When the vertical position of a snippet changes, we overlap the rectangles by this much.
    /// See the `SnippetShape` for a picture.
    pub overlap: f64,
    /// The largest `x` position (because logically we need to deal with infinite `x` positions
    /// but in practice we need to truncate).
    pub end_x: f64,
}

/// The result of laying out the snippets. The type parameter `T` is a snippet id (probably
/// `SnippetId` or `AudioSnippetId`).
pub struct Layout<T> {
    /// A map from the snippet's id to its shape.
    pub positions: HashMap<T, SnippetShape>,
    /// The maximum height of any snippet. This is redundant, in that it can be recomputed from
    /// `positions`.
    pub max_y: f64,
}

#[derive(Clone)]
pub struct SnippetBounds<T> {
    /// The time at which this snippet starts.
    start: Time,
    /// The time at which this snippet changes from thick to thin (if it does).
    thin: Option<Time>,
    /// The time at which this snippet ends (if it does).
    end: Option<Time>,
    id: T,
}

impl From<(SnippetId, &SnippetData)> for SnippetBounds<SnippetId> {
    fn from(data: (SnippetId, &SnippetData)) -> SnippetBounds<SnippetId> {
        let last_draw = data.1.last_draw_time();
        let thin = if let Some(end) = data.1.end_time() {
            if end <= last_draw {
                None
            } else {
                Some(last_draw)
            }
        } else {
            Some(last_draw)
        };
        SnippetBounds {
            start: data.1.start_time(),
            thin,
            end: data.1.end_time(),
            id: data.0,
        }
    }
}

impl From<(AudioSnippetId, &AudioSnippetData)> for SnippetBounds<AudioSnippetId> {
    fn from(data: (AudioSnippetId, &AudioSnippetData)) -> SnippetBounds<AudioSnippetId> {
        SnippetBounds {
            start: data.1.start_time(),
            thin: None,
            end: Some(data.1.end_time()),
            id: data.0,
        }
    }
}

pub fn layout<Id: Copy + Hash + Eq + Ord, T: Into<SnippetBounds<Id>>, I: Iterator<Item = T>>(
    iter: I,
    params: &Parameters,
) -> Layout<Id> {
    let mut sky = Skyline::new(params.end_x);
    let mut ret = Layout {
        positions: HashMap::new(),
        max_y: 0.0,
    };

    for b in iter.map(|t| t.into()) {
        let shape = sky.add_snippet(&b, params);
        ret.max_y = ret.max_y.max(
            shape
                .rects
                .iter()
                .map(|r| r.y1)
                .max_by(|x, y| x.partial_cmp(y).unwrap())
                .unwrap_or(0.0),
        );
        ret.positions.insert(b.id, shape);
    }

    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    // Creates a snippet that is empty, but has a starting and (possibly) an ending time.
    fn snip(id: usize, start: Time, thin: Option<Time>, end: Option<Time>) -> SnippetBounds<usize> {
        SnippetBounds {
            start,
            thin,
            end,
            id,
        }
    }

    fn sky(arr: &[(f64, f64)]) -> Skyline {
        Skyline {
            buildings: arr
                .iter()
                .map(|&(end_x, y)| Building { end_x, y })
                .collect(),
        }
    }

    macro_rules! snips {
        ( $(($begin:expr, $thin:expr, $end:expr)),* ) => {
            {
                let mut ret = Vec::<SnippetBounds<usize>>::new();
                let mut id = 0;
                $(
                    id += 1;
                    ret.push(snip(id, Time::from_micros($begin), $thin.map(Time::from_micros), $end.map(Time::from_micros)));
                )*
                ret.into_iter()
            }
        }
    }

    const PARAMS: Parameters = Parameters {
        thick_height: 2.0,
        thin_height: 1.0,
        h_padding: 0.0,
        v_padding: 0.0,
        min_width: 2.0,
        overlap: 1.0,
        pixels_per_usec: 1.0,
        end_x: 100.0,
    };

    const PARAMS_PADDED: Parameters = Parameters {
        thick_height: 2.0,
        thin_height: 1.0,
        h_padding: 1.0,
        v_padding: 1.0,
        min_width: 2.0,
        overlap: 1.0,
        pixels_per_usec: 1.0,
        end_x: 100.0,
    };

    #[test]
    fn layout_infinite() {
        let snips = snips!((0, Some(30), None), (10, Some(50), None));
        let layout = layout(snips, &PARAMS);
        assert_eq!(
            &layout.positions[&1].rects,
            &[
                Rect::new(0.0, 0.0, 31.0, 2.0),
                Rect::new(30.0, 0.0, 100.0, 1.0)
            ]
        );
        assert_eq!(
            &layout.positions[&2].rects,
            &[
                Rect::new(10.0, 2.0, 32.0, 4.0),
                Rect::new(31.0, 1.0, 51.0, 3.0),
                Rect::new(50.0, 1.0, 100.0, 2.0)
            ]
        );
    }

    #[test]
    fn layout_two() {
        let snips = snips!((0, Some(20), Some(50)), (20, Some(30), Some(50)));
        let layout = layout(snips, &PARAMS);
        assert_eq!(
            &layout.positions[&1].rects,
            &[
                Rect::new(0.0, 0.0, 21.0, 2.0),
                Rect::new(20.0, 0.0, 50.0, 1.0)
            ]
        );
        assert_eq!(
            &layout.positions[&2].rects,
            &[
                Rect::new(20.0, 2.0, 23.0, 4.0),
                Rect::new(22.0, 1.0, 31.0, 3.0),
                Rect::new(30.0, 1.0, 50.0, 2.0)
            ]
        );
    }

    #[test]
    fn layout_padded() {
        let snips = snips!((0, Some(20), Some(50)), (10, Some(30), Some(50)));
        let layout = layout(snips, &PARAMS_PADDED);
        assert_eq!(
            &layout.positions[&1].rects,
            &[
                Rect::new(0.0, 1.0, 21.0, 3.0),
                Rect::new(20.0, 1.0, 50.0, 2.0)
            ]
        );
        assert_eq!(
            &layout.positions[&2].rects,
            &[
                Rect::new(10.0, 4.0, 23.0, 6.0),
                Rect::new(22.0, 3.0, 31.0, 5.0),
                Rect::new(30.0, 3.0, 50.0, 4.0)
            ]
        );
    }

    #[test]
    fn instant_draw() {
        let snips = snips!((0, Some(0), Some(20)));
        let layout = layout(snips, &PARAMS);
        assert_eq!(
            &layout.positions[&1].rects,
            &[
                Rect::new(0.0, 0.0, 3.0, 2.0),
                Rect::new(2.0, 0.0, 20.0, 1.0),
            ]
        );

        let snips = snips!((0, None, Some(50)), (49, Some(49), Some(80)));
        let layout = self::layout(snips, &PARAMS);
        assert_eq!(
            &layout.positions[&1].rects,
            &[Rect::new(0.0, 0.0, 50.0, 2.0),]
        );
        assert_eq!(
            &layout.positions[&2].rects,
            &[
                Rect::new(49.0, 2.0, 52.0, 4.0),
                Rect::new(51.0, 0.0, 80.0, 1.0),
            ]
        );
    }

    #[test]
    fn fill_gaps() {
        let min_width = 3.0;
        let mut no_gaps = sky(&[(5.0, 1.0), (10.0, 2.0), (15.0, 1.0)]);
        let clone = no_gaps.clone();
        no_gaps.fill_gaps(min_width);
        assert_eq!(no_gaps, clone);

        let mut gap_start = sky(&[(1.0, 0.0), (3.0, 1.0)]);
        gap_start.fill_gaps(min_width);
        assert_eq!(gap_start, sky(&[(3.0, 1.0)]));

        let mut gap_start = sky(&[(1.0, 1.0), (3.0, 0.0)]);
        gap_start.fill_gaps(min_width);
        assert_eq!(gap_start, sky(&[(3.0, 1.0)]));

        let mut gap_mid = sky(&[(4.0, 2.0), (6.0, 1.0), (9.0, 3.0)]);
        gap_mid.fill_gaps(min_width);
        assert_eq!(gap_mid, sky(&[(4.0, 2.0), (9.0, 3.0)]));

        let mut gap_mid = sky(&[(4.0, 3.0), (6.0, 1.0), (9.0, 2.0)]);
        gap_mid.fill_gaps(min_width);
        assert_eq!(gap_mid, sky(&[(6.0, 3.0), (9.0, 2.0)]));

        let mut gap_end = sky(&[(5.0, 0.0), (6.0, 1.0)]);
        gap_end.fill_gaps(min_width);
        assert_eq!(gap_end, sky(&[(5.0, 0.0), (8.0, 1.0)]));

        let mut gap_end = sky(&[(5.0, 1.0), (6.0, 0.0)]);
        gap_end.fill_gaps(min_width);
        assert_eq!(gap_end, sky(&[(6.0, 1.0)]));

        let mut staircase = sky(&[(1.0, 1.0), (2.0, 2.0), (3.0, 3.0), (4.0, 4.0), (5.0, 5.0)]);
        staircase.fill_gaps(min_width);
        assert_eq!(staircase, sky(&[(3.0, 3.0), (6.0, 5.0)]));

        // There's a bit of asymmetry here with the way that we process things greedily
        // left-to-right.
        let mut staircase = sky(&[(1.0, 5.0), (2.0, 4.0), (3.0, 3.0), (4.0, 2.0), (5.0, 1.0)]);
        staircase.fill_gaps(min_width);
        assert_eq!(staircase, sky(&[(5.0, 5.0)]));
    }

    #[test]
    fn add_rect() {
        let min_width = 3.0;
        let mut s = sky(&[(100.0, 0.0)]);
        let mut new_s = Skyline::default();
        s.add_rect(10.0, 20.0, 1.0, min_width, &mut new_s);
        assert_eq!(new_s, sky(&[(20.0, 1.0)]));
        s.update_skyline(10.0, &new_s.buildings);
        s.fill_gaps(min_width);
        assert_eq!(s, sky(&[(10.0, 0.0), (20.0, 1.0), (100.0, 0.0)]));

        new_s.buildings.clear();
        s.add_rect(15.0, 25.0, 1.0, min_width, &mut new_s);
        assert_eq!(new_s, sky(&[(20.0, 2.0), (25.0, 1.0)]));
    }
}
