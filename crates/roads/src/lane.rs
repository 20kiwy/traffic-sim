//! RoadLane — the GRG AI path system (`nodes/road_lane.gd`).
//!
//! Every road segment spawns one lane polyline per traffic lane, offset
//! laterally from the segment centerline by half a lane-width steps.
//! Lanes are chained with the exact GRG tag convention:
//! `lane_prior_tag` / `lane_next_tag` = `R0..Rn` / `F0..Fn` (counted from
//! the divider outward), and `lane_next` / `lane_prior` link consecutive
//! segments whose tags match (the `RoadContainer::update_lane_seg_connections`
//! pass). `lane_left` / `lane_right` wire same-side neighbours for lane
//! changes. Transition (merge/split) lanes appear where the two segment
//! endpoints disagree on lane count, tagged with GRG's `a` (add) / `r`
//! (remove) suffixes.
//!
//! Agents (RoadLaneAgent) walk the polyline by arc length and hop to
//! `lane_next` at the end — this is what the traffic sim + MCP
//! `simulate` / `check_lanes` tools use to prove "cars will pass".

use crate::curve;
use crate::model::{RoadCurve, RoadPoint};
use gdmath::vec3::Vec3;

/// One generated AI lane (GRG `RoadLane extends Path3D`).
#[derive(Clone, Debug)]
pub struct RoadLane {
    /// stable name used by links: `<container>#{seg}:{tag}` e.g. "Road 1#2:F1"
    pub name: String,
    /// container (curve) index in the scene
    pub container: usize,
    /// segment index (between point i and i+1)
    pub segment: usize,
    /// 'F' or 'R' — travel direction relative to the curve's point order
    pub side: char,
    /// lane index within the side, 0 = adjacent to the divider
    pub index: usize,
    /// polyline samples in meters (x, z) following the curve direction
    pub pts: Vec<(f64, f64)>,
    /// elevation per polyline sample
    pub y: Vec<f64>,
    pub lane_next_tag: String,
    pub lane_prior_tag: String,
    /// resolved lane names (filled by `connect_lanes`)
    pub lane_next: Option<String>,
    pub lane_prior: Option<String>,
    pub lane_left: Option<String>,
    pub lane_right: Option<String>,
    /// GRG transition lane: lane count changes across the segment, so this
    /// lane tapers (added or removed). Rendered/validated differently.
    pub transition: bool,
    pub width: f64,
}

impl RoadLane {
    /// Arc length in meters.
    pub fn length_m(&self) -> f64 {
        let mut acc = 0.0f64;
        for i in 1..self.pts.len() {
            let (ax, az) = self.pts[i - 1];
            let (bx, bz) = self.pts[i];
            acc += ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt();
        }
        acc
    }

    /// Position + heading (radians) at arc distance `s` meters.
    pub fn sample_at(&self, s_m: f64) -> Option<(Vec3, f64)> {
        if self.pts.len() < 2 {
            return None;
        }
        let mut cum = vec![0.0f64];
        for i in 1..self.pts.len() {
            let (ax, az) = self.pts[i - 1];
            let (bx, bz) = self.pts[i];
            cum.push(cum[i - 1] + ((bx - ax).powi(2) + (bz - az).powi(2)).sqrt());
        }
        let total = *cum.last().unwrap();
        if total <= 1e-9 {
            let (x, z) = self.pts[0];
            return Some((Vec3::new(x as f32, self.y[0] as f32, z as f32), 0.0));
        }
        let d = s_m.clamp(0.0, total);
        let mut i = 1usize;
        while i < cum.len() - 1 && cum[i] < d {
            i += 1;
        }
        let seg = (cum[i] - cum[i - 1]).max(1e-9);
        let t = (d - cum[i - 1]) / seg;
        let (ax, az) = self.pts[i - 1];
        let (bx, bz) = self.pts[i];
        let x = ax + (bx - ax) * t;
        let z = az + (bz - az) * t;
        let y = self.y[i - 1] + (self.y[i] - self.y[i - 1]) * t;
        let yaw = (bz - az).atan2(bx - ax);
        Some((Vec3::new(x as f32, y as f32, z as f32), yaw))
    }

    /// Center offset (meters, signed from the road centerline; negative =
    /// left / REVERSE side) where this lane sits.
    pub fn center_offset(&self, lane_width: f64) -> f64 {
        let sign = if self.side == 'R' { 1.0 } else { -1.0 };
        sign * (0.5 + self.index as f64) * lane_width
    }
}

/// Generate all RoadLanes for a set of containers (GRG
/// `RoadSegment::generate_lane_segments` generalized per curve).
///
/// `sample_m` controls the polyline density (use the container's effective
/// density). One lane per through traffic lane + transition lanes for
/// tapering (merge/split) segments.
pub fn generate_lanes(curves: &[RoadCurve], sample_m: f64) -> Vec<RoadLane> {
    let mut out = Vec::new();
    for (ci, c) in curves.iter().enumerate() {
        if !c.generate_ai_lanes || c.points.len() < 2 {
            continue;
        }
        let seg_count = if c.closed { c.points.len() } else { c.points.len() - 1 };
        for si in 0..seg_count {
            let a = &c.points[si];
            let b = &c.points[(si + 1) % c.points.len()];
            lane_set_for_segment(c, ci, si, a, b, sample_m, &mut out);
        }
    }
    connect_lanes(&mut out, curves);
    out
}

/// Emit the lanes of ONE segment with per-side counts interpolated between
/// the two endpoint points (GRG `_match_lanes` + `offset_curve`).
fn lane_set_for_segment(
    c: &RoadCurve,
    ci: usize,
    si: usize,
    a: &RoadPoint,
    b: &RoadPoint,
    sample_m: f64,
    out: &mut Vec<RoadLane>,
) {
    let n_points = c.points.len();
    let closed = c.closed;
    // segment flatten (reuses the mesh pipeline's curve math)
    let seg = curve::segment_cubic(a, b);
    let (sp, sc, seg_len) = curve::flatten_cubic(&seg, sample_m.max(2.0));
    if seg_len < 1e-6 {
        return;
    }
    let width_a = a.lane_width;
    let width_b = b.lane_width;

    // through counts (min of both ends) + transition extras
    let a_fwd = a.fwd_lanes();
    let a_rev = a.rev_lanes;
    let b_fwd = b.fwd_lanes();
    let b_rev = b.rev_lanes;
    let fwd_through = a_fwd.min(b_fwd);
    let rev_through = a_rev.min(b_rev);
    let fwd_extra = a_fwd.max(b_fwd) - fwd_through; // lanes that taper
    let rev_extra = a_rev.max(b_rev) - rev_through;

    // The curve may run "backwards" relative to travel: GRG flips when two
    // nexts/priors face each other; here the segment direction is a -> b,
    // FORWARD lanes travel a -> b, REVERSE lanes travel b -> a.
    for side in ['F', 'R'] {
        let (through, extra, widening) = match side {
            'F' => (fwd_through, fwd_extra, b_fwd > a_fwd),
            _ => (rev_through, rev_extra, b_rev > a_rev),
        };
        for i in 0..(through + extra) {
            let transition = i >= through;
            // lane center offsets at both ends (from the centerline)
            let sign = if side == 'R' { -1.0 } else { 1.0 };
            let off_start = sign * (0.5 + i as f64) * width_a;
            let off_end = if transition {
                // taper onto/out of the outermost through lane
                let anchor = sign * (0.5 + (through.saturating_sub(1)) as f64) * width_b;
                if widening {
                    // lane ADDED at b: starts collapsed near the anchor
                    anchor * 0.85
                } else {
                    // lane REMOVED at b: collapses onto the anchor
                    anchor
                }
            } else {
                sign * (0.5 + i as f64) * width_b
            };
            let mut pts: Vec<(f64, f64)> = Vec::with_capacity(sp.len());
            let mut ys: Vec<f64> = Vec::with_capacity(sp.len());
            for (k, p) in sp.iter().enumerate() {
                let t = if seg_len > 1e-9 { (sc[k] / seg_len).clamp(0.0, 1.0) } else { 0.0 };
                let off = off_start + (off_end - off_start) * t;
                // perpendicular to the local tangent
                let prev = sp[k.saturating_sub(1)];
                let next = sp[(k + 1).min(sp.len() - 1)];
                let dx = next.x - prev.x;
                let dz = next.y - prev.y;
                let len = dx.hypot(dz).max(1e-9);
                // right of travel = (-sinθ, cosθ) with (cosθ, sinθ) = d/len
                let (rx, rz) = (-dz / len, dx / len);
                pts.push((p.x + rx * off, p.y + rz * off));
                let (ea, eb) = (a.elev, b.elev);
                ys.push(ea + (eb - ea) * t);
            }
            let tag = format!("{side}{i}");
            let name = format!("{}#{}:{}", c.name, si, tag);
            out.push(RoadLane {
                name,
                container: ci,
                segment: si,
                side,
                index: i,
                pts,
                y: ys,
                lane_next_tag: tag.clone(),
                lane_prior_tag: tag.clone(),
                lane_next: None,
                lane_prior: None,
                lane_left: None,
                lane_right: None,
                // both ADD (widening) and REM (collapsing) halves are
                // transition lanes — GRG's TRANSITION_ADD / TRANSITION_REM
                transition,
                width: width_a.min(width_b),
            });
            // GRG transition suffixes: 'a' = lane added after the point,
            // 'r' = lane removed after the point (matched by base tag).
            if transition && widening {
                if let Some(l) = out.last_mut() {
                    l.lane_next_tag = format!("{tag}a");
                    l.lane_prior_tag = format!("{tag}a");
                }
            } else if transition {
                if let Some(l) = out.last_mut() {
                    l.lane_next_tag = format!("{tag}r");
                    l.lane_prior_tag = format!("{tag}r");
                }
            }
        }
    }
    let _ = (n_points, closed);
}

/// GRG `RoadContainer::update_lane_seg_connections` — match tags across
/// consecutive segments of the same container and wire
/// lane_next / lane_prior / lane_left / lane_right.
///
/// Matching uses the BASE tag (suffix `a`/`r` stripped), so a lane ADDED
/// at a point (`F1a`, the segment-side end of a taper) connects to the
/// same-index lane REMOVED after it (`F1r`) — the two halves of one
/// physical transition lane — and plain through tags match directly.
pub fn connect_lanes(lanes: &mut [RoadLane], curves: &[RoadCurve]) {
    use std::collections::HashMap;
    let base = |tag: &str| -> String { tag.trim_end_matches(['a', 'r']).to_string() };
    // index: (container, segment, base tag) -> lane position
    let mut by_pos: HashMap<(usize, usize, String), usize> = HashMap::new();
    for (i, l) in lanes.iter().enumerate() {
        by_pos.insert((l.container, l.segment, base(&l.lane_prior_tag)), i);
    }
    let seg_counts: Vec<usize> = curves
        .iter()
        .map(|c| if c.closed { c.points.len() } else { c.points.len().saturating_sub(1) })
        .collect();

    // lane_next: segment si -> si+1 (same container). The GRG convention:
    // FORWARD lanes keep their tag going ahead; REVERSE lanes travel
    // backwards, so their "next" is the PREVIOUS segment with same tag.
    for i in 0..lanes.len() {
        let (ci, si, tag, side) = {
            let l = &lanes[i];
            (l.container, l.segment, base(&l.lane_next_tag), l.side)
        };
        let n_seg = seg_counts.get(ci).copied().unwrap_or(0);
        if n_seg == 0 {
            continue;
        }
        let closed = curves.get(ci).map(|c| c.closed).unwrap_or(false);
        let target: Option<usize> = match side {
            'F' => {
                if si + 1 < n_seg {
                    Some(si + 1)
                } else if closed && n_seg > 1 {
                    Some((si + 1) % n_seg)
                } else {
                    None
                }
            }
            _ => {
                if si > 0 {
                    Some(si - 1)
                } else if closed && n_seg > 1 {
                    Some(n_seg - 1)
                } else {
                    None
                }
            }
        };
        if let Some(ts) = target {
            if let Some(&j) = by_pos.get(&(ci, ts, tag.clone())) {
                if lanes[j].side == side && j != i && lanes[i].lane_next.is_none() {
                    lanes[i].lane_next = Some(lanes[j].name.clone());
                }
            }
        }
    }
    // lane_prior = the reverse of lane_next
    let names: Vec<String> = lanes.iter().map(|l| l.name.clone()).collect();
    for i in 0..lanes.len() {
        if let Some(next_name) = lanes[i].lane_next.clone() {
            if let Some(j) = names.iter().position(|n| *n == next_name) {
                if lanes[j].lane_prior.is_none() {
                    lanes[j].lane_prior = Some(names[i].clone());
                }
            }
        }
    }
    // lane_left / lane_right within the same segment + side
    for i in 0..lanes.len() {
        let (ci, si, side, idx) = (lanes[i].container, lanes[i].segment, lanes[i].side, lanes[i].index);
        for j in 0..lanes.len() {
            if i == j {
                continue;
            }
            let l = &lanes[j];
            if l.container == ci && l.segment == si && l.side == side {
                if l.index == idx + 1 {
                    // higher index = further from divider = further RIGHT
                    // for F lanes; for R lanes (traveling opposite) higher
                    // index is further LEFT.
                    if side == 'F' {
                        lanes[i].lane_right = Some(names[j].clone());
                    } else {
                        lanes[i].lane_left = Some(names[j].clone());
                    }
                } else if idx > 0 && l.index == idx - 1 {
                    if side == 'F' {
                        lanes[i].lane_left = Some(names[j].clone());
                    } else {
                        lanes[i].lane_right = Some(names[j].clone());
                    }
                }
            }
        }
    }
}

/// GRG `find_nearest_lane` — brute-force nearest lane within `max_dist`.
pub fn find_nearest_lane<'a>(lanes: &'a [RoadLane], x: f64, z: f64, max_dist: f64) -> Option<&'a RoadLane> {
    let mut best: Option<(f64, &RoadLane)> = None;
    for l in lanes {
        for (px, pz) in &l.pts {
            let d = ((px - x).powi(2) + (pz - z).powi(2)).sqrt();
            if d <= max_dist && best.map(|(bd, _)| d < bd).unwrap_or(true) {
                best = Some((d, l));
            }
        }
    }
    best.map(|(_, l)| l)
}

/// Lane continuity report (the MCP `check_lanes` tool): per container,
/// how many lanes, how many have a `lane_next` (can cars keep going?),
/// and the list of dead-end tags.
#[derive(Clone, Debug, Default)]
pub struct LaneCheck {
    pub container: String,
    pub lanes: usize,
    pub connected: usize,
    pub transitions: usize,
    /// dead ends at open container ends (expected — the road simply ends)
    pub expected_dead_ends: usize,
    /// dead ends mid-network (a car driving here gets stuck)
    pub unexpected_dead_ends: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct LaneReport {
    pub total_lanes: usize,
    pub containers: Vec<LaneCheck>,
}

pub fn check_lanes(lanes: &[RoadLane], curves: &[RoadCurve]) -> LaneReport {
    let mut rep = LaneReport { total_lanes: lanes.len(), containers: Vec::new() };
    for (ci, c) in curves.iter().enumerate() {
        let cl: Vec<&RoadLane> = lanes.iter().filter(|l| l.container == ci).collect();
        if cl.is_empty() {
            continue;
        }
        let with_next = cl.iter().filter(|l| l.lane_next.is_some()).count();
        let transitions = cl.iter().filter(|l| l.transition).count();
        let closed = c.closed;
        let seg_n = if closed { c.points.len() } else { c.points.len().saturating_sub(1) };
        let mut expected = 0usize;
        let mut unexpected = Vec::new();
        for l in cl.iter().filter(|l| l.lane_next.is_none() && !l.transition) {
            let at_open_end = match l.side {
                'F' => l.segment == seg_n - 1 && !closed,
                _ => l.segment == 0 && !closed,
            };
            if at_open_end {
                expected += 1;
            } else if unexpected.len() < 12 {
                unexpected.push(format!("{}{} @seg{}", l.side, l.index, l.segment));
            }
        }
        rep.containers.push(LaneCheck {
            container: c.name.clone(),
            lanes: cl.len(),
            connected: with_next,
            transitions,
            expected_dead_ends: expected,
            unexpected_dead_ends: unexpected,
        });
    }
    rep
}

// serde_json is only needed here for the report — the roads crate does not
// otherwise depend on it; gate behind the existing workspace dependency.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::RoadPoint;

    fn straight(n: usize, lanes: usize, rev: usize) -> RoadCurve {
        let mut c = RoadCurve::new("Test");
        for i in 0..n {
            let mut p = RoadPoint::new(i as f64 * 40.0, 0.0);
            p.lanes = lanes;
            p.rev_lanes = rev;
            p.lane_width = 4.0;
            c.add_point(p);
        }
        c
    }

    #[test]
    fn lanes_generated_and_connected() {
        let curves = vec![straight(3, 4, 2)];
        let lanes = generate_lanes(&curves, 8.0);
        // 2 segments × (2 F + 2 R) = 8
        assert_eq!(lanes.len(), 8, "expected 8 lanes");
        let fwd: Vec<_> = lanes.iter().filter(|l| l.side == 'F').collect();
        assert_eq!(fwd.len(), 4);
        // F0 of seg0 connects to F0 of seg1
        let f0s0 = lanes.iter().find(|l| l.side == 'F' && l.index == 0 && l.segment == 0).unwrap();
        assert!(f0s0.lane_next.is_some(), "through lane should have lane_next");
        let next = lanes.iter().find(|l| Some(l.name.clone()) == f0s0.lane_next).unwrap();
        assert_eq!(next.segment, 1);
        assert_eq!(next.index, 0);
        // report parses
        let rep = check_lanes(&lanes, &curves);
        assert_eq!(rep.total_lanes, 8);
    }

    #[test]
    fn taper_segment_makes_transitions() {
        let mut c = RoadCurve::new("Taper");
        let mut a = RoadPoint::new(0.0, 0.0);
        a.lanes = 4;
        a.rev_lanes = 2;
        a.lane_width = 4.0;
        let mut b = RoadPoint::new(80.0, 0.0);
        b.lanes = 2;
        b.rev_lanes = 1;
        b.lane_width = 4.0;
        c.add_point(a);
        c.add_point(b);
        let lanes = generate_lanes(&[c], 8.0);
        let trans = lanes.iter().filter(|l| l.transition).count();
        assert!(trans >= 2, "expected >=2 transition lanes for 4->2 taper, got {trans}");
    }
}
