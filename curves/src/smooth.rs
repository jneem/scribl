use druid::{
    kurbo::{BezPath, Point},
    Vec2,
};

/// Turns a polyline into a (mostly) smooth curve through the same points.
/// The returned curve will consist only of cubic segments.
///
/// The magnitude of the tangents of the returned curve are controlled by `tangent_factor`.
/// A reasonable default value is `0.4`; higher values will smooth out the corners more, but
/// at the cost of possibly introducing loops or other artifacts between them.
pub fn smooth(points: &[Point], tangent_factor: f64) -> BezPath {
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
    let mut tang = points[2] - points[0];
    tang /= tang.hypot() + 1e-8;
    let tang_mag = (points[1] - points[0]).hypot() * tangent_factor;
    ret.curve_to(points[0], points[1] - tang * tang_mag, points[1]);

    let mut prev_tangent = tang;

    for w in points.windows(3).skip(1) {
        let prev = w[0];
        let cur = w[1];
        let next = w[2];

        let delta = prev - cur;
        let dist = delta.hypot();

        let tang = next - prev;
        let tang = tang / (tang.hypot() + 1e-8);

        // The basic magnitude of the tangent is `tangent_factor` times the distance.
        let mut tang_mag = dist * tangent_factor;
        let mut prev_tang_mag = dist * tangent_factor;

        // Don't allow either tangent to stick out too much. This prevents "bulges" that would
        // otherwise appear when we're just below the angle threshold. For example, in this
        // configuration
        //
        // x----------------x
        //                  |
        //                  |
        //                  |
        //                  |
        //                  x
        //
        //  we don't want to make a giant smooth tangent at the corner.
        let orthog = Vec2::new(-delta.y, delta.x);
        let orthog = orthog / orthog.hypot();
        let orthog_mag = orthog.dot(tang).abs();
        let prev_orthog_mag = orthog.dot(prev_tangent).abs();
        let mag_threshold = (orthog_mag.min(prev_orthog_mag) * 1.5).max(0.1);
        if orthog_mag > mag_threshold {
            tang_mag *= mag_threshold / orthog_mag;
        } else if prev_orthog_mag > mag_threshold {
            prev_tang_mag *= mag_threshold / prev_orthog_mag;
        }

        ret.curve_to(
            prev + prev_tangent * prev_tang_mag,
            cur - tang * tang_mag,
            cur,
        );
        prev_tangent = tang;
    }

    let last = points[points.len() - 1];
    let prev = points[points.len() - 2];
    let prev_tangent_mag = (last - prev).hypot() * tangent_factor;
    ret.curve_to(prev + prev_tangent * prev_tangent_mag, last, last);

    ret
}
