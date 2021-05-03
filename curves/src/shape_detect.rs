use druid::kurbo::{BezPath, Point, Vec2};
use std::f64::consts::PI;

use crate::{StrokeInProgress, Time};

const MAX_LINE_DEVIATION: f64 = 0.05;
const ANGLE_TOLERANCE: f64 = 5.0 * PI / 180.0;
const ANGLE_DEGREES: [f64; 9] = [-180.0, -135.0, -90.0, -45.0, 0.0, 45.0, 90.0, 135.0, 180.0];

pub(crate) struct Shape {
    pub path: BezPath,
    pub times: Vec<Time>,
}

pub(crate) fn detect(stroke: &StrokeInProgress) -> Option<Shape> {
    detect_line(stroke)
}

fn detect_line(stroke: &StrokeInProgress) -> Option<Shape> {
    let points = stroke.points.borrow();
    if points.len() < 2 {
        return None;
    }

    let start = points[0];
    let end = points.last().unwrap();
    let dist = start.distance(*end);
    if dist < 1e-6 {
        return None;
    }

    let tang = (*end - start) / dist;

    // Compute the distance from a point to the start -> end line segment.
    let d = |p: &Point| -> f64 {
        let p = *p - start;
        let tang_component = p.dot(tang);
        let residual = p - tang_component * tang;
        let tang_extra = if tang_component < 0.0 {
            tang_component
        } else if tang_component > dist {
            tang_component - dist
        } else {
            0.0
        };
        (residual.hypot2() + tang_extra.powi(2)).sqrt()
    };

    if points.iter().any(|p| d(p) > dist * MAX_LINE_DEVIATION) {
        None
    } else {
        let angle = snap_angle(tang.atan2());
        let tang = Vec2::from_angle(angle);
        let end = start + tang * dist;

        let mut path = BezPath::new();
        path.move_to(start);
        path.line_to(end);
        let times = stroke.times.borrow();
        assert!(times.len() > 2);

        // TODO: snap to angles
        Some(Shape {
            path,
            times: vec![times[0], *times.last().unwrap()],
        })
    }
}

// `angle` is assumed to be between -\pi and \pi.
fn snap_angle(angle: f64) -> f64 {
    for &th in &ANGLE_DEGREES {
        let th = th * PI / 180.0;
        if (angle - th).abs() < ANGLE_TOLERANCE {
            return th;
        }
    }
    angle
}
