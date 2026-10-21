//! Cubic Bezier + arc-length flattening — direct port of
//! godot-road-generator's road_segment.gd `_update_curve` and Godot
//! Curve3D's arc-length baking, as validated in the TypeScript prototype
//! and the previous compile-verified Rust build.

use crate::model::RoadPoint;
use gdmath::vec2::Vec2;

#[derive(Clone, Copy, Debug)]
pub struct CubicSeg {
    pub p0: Vec2,
    /// first control (start + out-handle)
    pub p1: Vec2,
    /// second control (end - in-handle)
    pub p2: Vec2,
    pub p3: Vec2,
}

pub const EPS: f64 = 1e-12;

/// Godot's ease(x, -2): 2x² for x<0.5 else 1-2(1-x)² — pins ends.
pub fn ease_neg2(t: f64) -> f64 {
    if t < 0.5 {
        2.0 * t * t
    } else {
        1.0 - 2.0 * (1.0 - t) * (1.0 - t)
    }
}

pub fn bez_point(s: &CubicSeg, t: f64) -> Vec2 {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    Vec2::new(
        a * s.p0.x + b * s.p1.x + c * s.p2.x + d * s.p3.x,
        a * s.p0.y + b * s.p1.y + c * s.p2.y + d * s.p3.y,
    )
}

/// Control points of the cubic between two RoadPoints (GRG):
/// out-handle = start + dir·next_mag, in-handle = end − dir·prior_mag.
pub fn segment_cubic(a: &RoadPoint, b: &RoadPoint) -> CubicSeg {
    let (ax, az) = a.dir();
    let (bx, bz) = b.dir();
    // R14: clamp the bezier handles to the chord. Magnitudes larger than
    // the segment distance used to loop the spline back over itself —
    // OSM slip ramps carried 30 m handles on ~4.5 m point spacing and
    // scribbled into folded dark fans, and user-placed points did the
    // same (the "point placement bug"). 0.6 of the chord is a generous
    // ceiling: normal GRG magnitudes run ~1/3 of the segment.
    let chord = ((b.x - a.x).hypot(b.z - a.z)) * 0.6;
    let out_m = a.next_mag.min(chord);
    let in_m = b.prior_mag.min(chord);
    CubicSeg {
        p0: Vec2::new(a.x, a.z),
        p1: Vec2::new(a.x + ax * out_m, a.z + az * out_m),
        p2: Vec2::new(b.x - bx * in_m, b.z - bz * in_m),
        p3: Vec2::new(b.x, b.z),
    }
}

/// Arc-length flattening of a cubic at `density` (same units as inputs,
/// i.e. meters). Returns points [P0..P3]-ish inclusive with cumulative
/// lengths and total length.
pub fn flatten_cubic(s: &CubicSeg, density: f64) -> (Vec<Vec2>, Vec<f64>, f64) {
    const N: usize = 24;
    let mut samples: Vec<Vec2> = Vec::with_capacity(N + 1);
    samples.push(s.p0);
    let mut length = 0.0f64;
    let mut coarse: Vec<f64> = vec![0.0];
    for i in 1..=N {
        let p = bez_point(s, i as f64 / N as f64);
        length += p.dist(*samples.last().unwrap());
        samples.push(p);
        coarse.push(length);
    }
    if length < EPS {
        let straight = s.p0.dist(s.p3);
        return (vec![s.p0, s.p3], vec![0.0, straight], length);
    }

    let n_loops = ((length / density) as usize).max(1);
    let step = length / n_loops as f64;
    let mut pts: Vec<Vec2> = vec![s.p0];
    let mut cum: Vec<f64> = vec![0.0];
    let mut target = step;
    let mut seg_idx = 1usize;
    for _i in 1..=n_loops {
        while seg_idx < coarse.len() - 1 && coarse[seg_idx] < target {
            seg_idx += 1;
        }
        let t0 = (seg_idx - 1) as f64 / N as f64;
        let t1 = seg_idx as f64 / N as f64;
        let seg_len = (coarse[seg_idx] - coarse[seg_idx - 1]).max(EPS);
        let frac = (target - coarse[seg_idx - 1]) / seg_len;
        let t = t0 + (t1 - t0) * frac;
        pts.push(bez_point(s, t.min(1.0)));
        cum.push(target);
        target = step * pts.len() as f64;
    }
    let last = *pts.last().unwrap();
    if last.dist(s.p3) > EPS {
        pts.push(s.p3);
        cum.push(length);
    }
    (pts, cum, length)
}

/// Distance from point p to segment a-b.
pub fn dist_to_segment(px: f64, py: f64, ax: f64, ay: f64, bx: f64, by: f64) -> f64 {
    let dx = bx - ax;
    let dy = by - ay;
    let len2 = dx * dx + dy * dy;
    let mut t = 0.0;
    if len2 > EPS {
        t = (((px - ax) * dx + (py - ay) * dy) / len2).clamp(0.0, 1.0);
    }
    (px - (ax + t * dx)).hypot(py - (ay + t * dy))
}

/// Heading from a to b (xz plane).
pub fn heading_between(ax: f64, az: f64, bx: f64, bz: f64) -> f64 {
    (bz - az).atan2(bx - ax)
}
