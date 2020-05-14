use druid::kurbo::{BezPath, Point};

/// Turns a polyline into a (mostly) smooth curve through the same points.
/// The returned curve will consist only of cubic segments.
///
/// Points are dividing into "smooth" or "non-smooth" points depending on the angle between the
/// incoming and outgoing edges: if the angle is less than `angle_threshold` radians,
/// the point will not be smoothed, and the tangents of the returned curve will be in the same
/// direction as the original tangents.
///
/// The magnitude of the tangents of the returned curve are controlled by `tangent_factor`.
/// A reasonable default value is `0.4`; higher values will smooth out the corners more, but
/// at the cost of possibly introducing loops or other artifacts between them.
pub fn smooth(points: &[Point], tangent_factor: f64, angle_threshold: f64) -> BezPath {
    let mut ret = BezPath::new();

    if points.is_empty() {
        return ret;
    } else if points.len() == 1 {
        ret.move_to(points[0]);
        return ret;
    } else if points.len() == 2 {
        ret.move_to(points[0]);
        // A line_to would sort of make more sense here, but it's convenient elsewhere if we
        // enforce that the returned curve only consists of cubic segments.
        ret.curve_to(points[0], points[1], points[1]);
        return ret;
    }

    ret.move_to(points[0]);
    let mut prev_tangent = (points[1] - points[0]) * tangent_factor;

    for w in points.windows(3) {
        let prev = w[0];
        let cur = w[1];
        let next = w[2];

        // We find the angle formed by prev, cur, and next. If that angle is small enough, we make a "smooth" node.
        let d_prev = prev - cur;
        let d_next = next - cur;
        let prev_dist = cur.distance(prev);
        let next_dist = cur.distance(next);

        // Note that this will be unstable if the points are very close. We're relying on the fact that the points we get
        // here have already been simplified, so they won't be too close.
        let angle = (d_prev.dot(d_next) / (prev_dist * next_dist)).acos();

        // The angle is between 0 and pi. Close to pi means that prev -> cur -> next is "almost straight".
        if angle >= angle_threshold {
            // The tangent at the current point is parallel to the line from the previous point to the next point.
            let tang = next - prev;
            // The magnitude of the tangent is `tangent_factor` times the smaller of the distances.
            let tang = tang / tang.hypot() * prev_dist.min(next_dist) * tangent_factor;

            ret.curve_to(prev + prev_tangent, cur - tang, cur);
            prev_tangent = tang;
        } else {
            // The current segment and the next segment get two different tangents.
            let tang = -d_prev * tangent_factor;
            ret.curve_to(prev + prev_tangent, cur - tang, cur);
            prev_tangent = d_next * tangent_factor;
        }
    }

    let last = points[points.len() - 1];
    let prev = points[points.len() - 2];
    let last_tangent = (last - prev) * tangent_factor;
    ret.curve_to(prev + prev_tangent, last - last_tangent, last);

    ret
}

#[cfg(test)]
mod tests {
    use druid::kurbo::PathSeg;

    use super::*;

    #[test]
    fn test_angle_threshold() {
        let almost_straight: Vec<Point> =
            vec![(0.0, 0.0).into(), (1.0, 0.01).into(), (2.0, 0.0).into()];
        let smoothed = smooth(&almost_straight, 0.25, std::f64::consts::PI / 2.0);
        let seg_before = smoothed.get_seg(1).unwrap();
        let seg_after = smoothed.get_seg(2).unwrap();
        if let (PathSeg::Cubic(c0), PathSeg::Cubic(c1)) = (seg_before, seg_after) {
            assert!(((c0.p3 - c0.p2) - (c1.p1 - c1.p0)).hypot() <= 1e-6);
        } else {
            panic!();
        }

        let right_angle: Vec<Point> = vec![(0.0, 0.0).into(), (1.0, 0.0).into(), (1.0, 1.0).into()];
        let smoothed = smooth(&right_angle, 0.25, std::f64::consts::PI / 1.9);
        let seg_before = smoothed.get_seg(1).unwrap();
        let seg_after = smoothed.get_seg(2).unwrap();
        if let (PathSeg::Cubic(c0), PathSeg::Cubic(c1)) = (seg_before, seg_after) {
            assert!(((c0.p3 - c0.p2) - (c1.p1 - c1.p0)).hypot() >= 0.1);
        } else {
            panic!();
        }
    }
}
