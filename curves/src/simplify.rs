use druid::kurbo::{Line, Point};

// Squared distance from the point `p` to the line *segment* `line`.
fn sq_distance(p: Point, line: Line) -> f64 {
    // Translate the start of the line to the origin.
    let px = p.x - line.p0.x;
    let py = p.y - line.p0.y;

    let vx = line.p1.x - line.p0.x;
    let vy: f64 = line.p1.y - line.p0.y;

    let dot = px * vx + py * vy;
    let v_norm_sq = vx * vx + vy * vy;
    if dot <= 0.0 || v_norm_sq < 1e-6 {
        // line.p0 is the closest point to p (or close enough, if the line is very short)
        px * px + py * py
    } else if dot >= v_norm_sq {
        // line.p1 is the closest point to p
        (p.x - line.p1.x) * (p.x - line.p1.x) + (p.y - line.p1.y) * (p.y - line.p1.y)
    } else {
        // The closest point to p is somewhere in the middle. The projection of p in direction v
        // is q = <p, v> v / v_norm_sq
        let qx = dot * vx / v_norm_sq;
        let qy = dot * vy / v_norm_sq;
        (px - qx) * (px - qx) + (py - qy) * (py - qy)
    }
}

/// Given a polyline represented as a collection of points, returns a simpler polyline that
/// approximates the original. To be precise, the return value is a collection of indices into
/// `points`; the simpler polyline is made up of the subset of `points` indexed by the return
/// value.
pub fn simplify(points: &[Point], eps: f64) -> Vec<usize> {
    if points.is_empty() {
        return Vec::new();
    } else if points.len() == 1 {
        return vec![0];
    }

    // We're using the RDP algorithm, which is a recursive divide-and-conquer
    // algorithm. Each recursive call needs to know its beginning and ending
    // indices. Here we're managing our own stack of these recursive calls
    // in order to avoid using space on the "real" stack.
    let mut stack: Vec<(usize, usize)> = vec![(0, points.len() - 1)];

    // These will be set to true for the points we want to keep;
    let mut included = vec![false; points.len()];
    included[0] = true;
    *included.last_mut().unwrap() = true;

    let eps_sq = eps * eps;
    while let Some((start, end)) = stack.pop() {
        let line = Line::new(points[start], points[end]);

        // If we get `None` here, it just means that the range is empty, so there's no need to do anything.
        if let Some((max_idx, max_sq_dist)) = points[(start + 1)..end]
            .iter()
            .map(|p| sq_distance(*p, line))
            .enumerate()
            .max_by(|&(_idx1, dist1), &(_idx2, dist2)| {
                // We want to sort by the distance, but f64 doesn't implement Ord so we can't do
                // max_by_key.
                dist1
                    .partial_cmp(&dist2)
                    .unwrap_or(std::cmp::Ordering::Less)
            })
        {
            let max_idx = start + 1 + max_idx;
            if max_sq_dist > eps_sq {
                included[max_idx] = true;
                stack.push((start, max_idx));
                stack.push((max_idx, end));
            }
        }
    }

    included
        .into_iter()
        .enumerate()
        .filter(|(_idx, included)| *included)
        .map(|(idx, _included)| idx)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sq_distance() {
        let p = Point::new(0.0, 0.0);
        let q = Point::new(0.0, 1.0);
        let r = Point::new(1.0, 0.0);
        let s = Point::new(1.0, 1.0);

        assert_eq!(sq_distance(q, Line::new(p, p)), 1.0);
        assert_eq!(sq_distance(p, Line::new(p, p)), 0.0);
        assert_eq!(sq_distance(q, Line::new(p, q)), 0.0);
        assert_eq!(sq_distance(r, Line::new(p, q)), 1.0);
        assert_eq!(sq_distance(s, Line::new(p, q)), 1.0);
        assert_eq!(sq_distance(q, Line::new(p, s)), 0.5);
    }

    #[test]
    fn test_simplify() {
        fn pvec(ps: &[(f64, f64)]) -> Vec<Point> {
            ps.iter().map(|p| (*p).into()).collect()
        }

        assert_eq!(
            simplify(&pvec(&[(0.0, 0.0), (1.0, 0.0), (2.0, 0.0)]), 0.1),
            vec![0, 2],
        );

        assert_eq!(
            simplify(&pvec(&[(0.0, 0.0), (1.0, 0.01), (2.0, 0.0)]), 0.1),
            vec![0, 2],
        );

        assert_eq!(
            simplify(&pvec(&[(0.0, 0.0), (1.0, 0.11), (2.0, 0.0)]), 0.1),
            vec![0, 1, 2],
        );
    }
}
