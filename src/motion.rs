//! Human-looking pointer paths, played back as many small eased moves.

use std::num::NonZeroU32;

use crate::tape::Point;

/// `steps` points eased in and out from just after `from`, ending exactly at `to`.
#[must_use]
pub fn path(from: Point, to: Point, steps: NonZeroU32) -> Vec<Point> {
    let n = i128::from(steps.get());
    let scale = n * n * n;
    // Cubic ease-in-out, kept exact as a numerator over n³.
    let eased = |i: i128| {
        if 2 * i < n {
            4 * i * i * i
        } else {
            scale - 4 * (n - i) * (n - i) * (n - i)
        }
    };
    let lerp = |a: u32, b: u32, e: i128| {
        let (a, b) = (i128::from(a), i128::from(b));
        let value = a + (b - a) * e / scale;
        u32::try_from(value.clamp(a.min(b), a.max(b))).unwrap_or(0)
    };
    (1..=n)
        .map(|i| {
            let e = eased(i);
            Point {
                x: lerp(from.x, to.x, e),
                y: lerp(from.y, to.y, e),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(x: u32, y: u32) -> Point {
        Point { x, y }
    }

    fn n(steps: u32) -> NonZeroU32 {
        NonZeroU32::new(steps).unwrap()
    }

    #[test]
    fn has_one_point_per_step_and_ends_on_target() {
        let points = path(p(0, 0), p(1920, 1080), n(40));
        assert_eq!(points.len(), 40);
        assert_eq!(points.last(), Some(&p(1920, 1080)));
    }

    #[test]
    fn single_step_jumps_to_target() {
        assert_eq!(path(p(5, 5), p(100, 200), n(1)), vec![p(100, 200)]);
    }

    #[test]
    fn is_monotonic_in_both_directions() {
        for (from, to) in [(p(0, 1000), p(1500, 20)), (p(1500, 20), p(0, 1000))] {
            let points = path(from, to, n(60));
            let xs: Vec<u32> = std::iter::once(from.x)
                .chain(points.iter().map(|q| q.x))
                .collect();
            let ys: Vec<u32> = std::iter::once(from.y)
                .chain(points.iter().map(|q| q.y))
                .collect();
            let toward =
                |a: u32, b: u32, start: u32, end: u32| if end >= start { a <= b } else { a >= b };
            assert!(
                xs.windows(2).all(|w| toward(w[0], w[1], from.x, to.x)),
                "{xs:?}"
            );
            assert!(
                ys.windows(2).all(|w| toward(w[0], w[1], from.y, to.y)),
                "{ys:?}"
            );
        }
    }

    #[test]
    fn passes_halfway_at_half_time() {
        let points = path(p(0, 0), p(1000, 500), n(10));
        assert_eq!(points[4], p(500, 250));
    }

    #[test]
    fn eases_in_and_out() {
        let points = path(p(0, 0), p(1000, 0), n(20));
        let first = points[0].x;
        let middle = points[10].x - points[9].x;
        let last = points[19].x - points[18].x;
        assert!(
            first < middle && last < middle,
            "first {first}, middle {middle}, last {last}"
        );
    }

    #[test]
    fn standing_still_stays_put() {
        assert!(path(p(7, 9), p(7, 9), n(5)).iter().all(|q| *q == p(7, 9)));
    }
}
