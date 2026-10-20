//! Traffic simulation per intersection — vehicle agents that follow the
//! procedural RoadCurves (GRG's RoadLane AI idea, simplified), keep a
//! car-following gap, and obey per-junction signal phases with stop lines
//! at the crosswalks. Rendered as "vehicle" GLB props; the app animates
//! them every frame, the CLI renders deterministic snapshots (--sim S).

use crate::library::AssetLibrary;
use crate::props::PropInstance;
use crate::scene::PropDraw;
use crate::signals::{Control, SignalRegistry, SignalState};
use gdmath::vec3::Vec3;
use gdmath::Mat4;
use roads::model::RoadCurve;

#[derive(Clone, Copy, Debug)]
pub struct SimSettings {
    /// vehicles per km per direction
    pub density: f64,
    /// simulation seconds per real second
    pub time_scale: f64,
    /// free-flow speed (m/s) — ~50 km/h urban
    pub vmax: f64,
    pub seed: u64,
    /// left-hand traffic (Tanzania drives on the left) — flips the
    /// lateral lane offsets vehicles keep from the centerline
    pub left_hand: bool,
}

impl Default for SimSettings {
    fn default() -> Self {
        Self { density: 12.0, time_scale: 1.0, vmax: 13.5, seed: 42, left_hand: false }
    }
}

/// One vehicle on one curve: arc position, speed, direction, lane index.
#[derive(Clone, Copy, Debug)]
pub struct Vehicle {
    pub curve: usize,
    /// arc position along the curve (m)
    pub s: f64,
    /// signed speed (m/s): + travels s increasing, - travels s decreasing
    pub speed: f64,
    /// lateral offset from the centerline (m) — right-hand traffic
    pub lateral: f64,
    /// yaw offset for reverse travel (0 or π)
    pub flip: bool,
    /// color variation seed (scale jitter only — props are one mesh)
    pub scale: f64,
}

/// A stop line: junction approach on a curve at arc position `s`.
#[derive(Clone, Copy, Debug)]
struct StopLine {
    curve: usize,
    /// arc position of the junction along the curve
    s: f64,
    junction: usize,
    /// axis group of the approach: true = the junction's primary axis
    group: bool,
    /// true when this stop line controls vehicles traveling +s
    positive_dir: bool,
}

/// One rendered traffic-signal head: pole beside the stop line, facing
/// the approach. The Signals panel can re-position these.
#[derive(Clone, Copy, Debug)]
pub struct LightPose {
    pub x: f64,
    pub z: f64,
    pub elev: f64,
    /// approach travel bearing (head faces oncoming traffic)
    pub bearing: f64,
    pub junction: usize,
    /// primary-axis group of the head
    pub group: bool,
    /// true when this head serves the +s travel direction
    pub positive_dir: bool,
}

pub struct TrafficSim {
    pub vehicles: Vec<Vehicle>,
    pub settings: SimSettings,
    pub t: f64,
    stop_lines: Vec<StopLine>,
    /// cached curve lengths
    lengths: Vec<f64>,
    two_way: Vec<bool>,
    /// (curve, arc s) of every junction touch on each curve
    /// per-junction phase offsets (deterministic from position)
    phases: Vec<f64>,
    /// junction kinds (roundabouts never get heads)
    kinds: Vec<roads::junction::JunctionKind>,
    junction_pos: Vec<(f64, f64)>,
    /// signal registry snapshot (plan per junction index)
    plans: Vec<Option<crate::signals::SignalPlan>>,
    /// rendered signal heads (one per controlled approach)
    pub light_poses: Vec<LightPose>,
    rng: u64,
}

impl TrafficSim {
    /// Build the sim for a scene: spawn vehicles + locate stop lines.
    pub fn new(curves: &[RoadCurve], junctions: &[roads::junction::Junction], settings: SimSettings) -> Self {
        let lengths: Vec<f64> = curves.iter().map(|c| c.length_m()).collect();
        let two_way: Vec<bool> = curves
            .iter()
            .map(|c| !c.is_oneway())
            .collect();

        // stop lines: every junction leg maps to an approach on its curve
        let mut stop_lines: Vec<StopLine> = Vec::new();
        let mut phases: Vec<f64> = Vec::new();
        for (ji, j) in junctions.iter().enumerate() {
            // deterministic phase offset from position (0..20 s cycle)
            let ph = ((j.x * 0.37 + j.z * 0.61).rem_euclid(20.0)).abs() % 20.0;
            phases.push(ph);
            for leg in &j.legs {
                let Some(c) = curves.get(leg.curve) else { continue };
                let Some((s_j, _d)) = c.closest_s(j.x, j.z) else { continue };
                // approach direction: vehicles arrive moving OPPOSITE the
                // leg's outward dir; their travel tangent ≈ -out_dir
                let yaw = {
                    let (pts, _, yaws) = c.arc_table();
                    let mut best = (0.0f64, 0usize);
                    for (i, p) in pts.iter().enumerate() {
                        let d = ((p.x as f64 - j.x).hypot(p.z as f64 - j.z));
                        if d < best.0 {
                            best = (d, i);
                        }
                    }
                    yaws.get(best.1).copied().unwrap_or(0.0)
                };
                let tangent = (yaw.cos(), yaw.sin());
                // two approach sides per curve at this junction
                for positive_dir in [true, false] {
                    let travel = if positive_dir { tangent } else { (-tangent.0, -tangent.1) };
                    // does this travel direction approach the junction from
                    // this leg (i.e. travel ≈ -out_dir)?
                    let dot = travel.0 * leg.out_dir.0 + travel.1 * leg.out_dir.1;
                    if dot < -0.5 {
                        // axis group: compare the leg's outward angle to the
                        // junction's first leg (primary axis)
                        let primary = junctions[ji]
                            .legs
                            .first()
                            .map(|l| l.out_dir.1.atan2(l.out_dir.0))
                            .unwrap_or(0.0);
                        let a = leg.out_dir.1.atan2(leg.out_dir.0);
                        let diff = ((a - primary).abs().to_degrees().rem_euclid(180.0) - 90.0).abs();
                        let group = diff < 45.0; // near-parallel to primary axis
                        stop_lines.push(StopLine {
                            curve: leg.curve,
                            s: s_j,
                            junction: ji,
                            group,
                            positive_dir,
                        });
                    }
                }
            }
        }

        let mut sim = Self {
            vehicles: Vec::new(),
            settings,
            t: 0.0,
            stop_lines,
            lengths,
            two_way,
            phases,
            kinds: junctions.iter().map(|j| j.kind).collect(),
            junction_pos: junctions.iter().map(|j| (j.x, j.z)).collect(),
            plans: junctions.iter().map(|_| None).collect(),
            light_poses: Vec::new(),
            rng: settings.seed.wrapping_mul(0x9E3779B97F4A7C15).max(1),
        };
        sim.build_lights(curves, junctions);
        sim.spawn_initial(curves);
        sim
    }

    /// Attach a signal registry AFTER construction (the app rebuilds the
    /// sim when the registry edits — this keeps the old `new()` signature
    /// working for CLI / selftest callers).
    pub fn with_registry(mut self, registry: &SignalRegistry, junctions: &[roads::junction::Junction], curves: &[RoadCurve]) -> Self {
        let plans: Vec<Option<crate::signals::SignalPlan>> = junctions
            .iter()
            .map(|j| registry.plan_for_junction(j.x, j.z).cloned())
            .collect();
        self.plans = plans;
        self.build_lights(curves, junctions);
        self
    }

    /// Compute the rendered signal-head set for every CONTROLLED junction
    /// approach (roundabouts and uncontrolled junctions get none).
    fn build_lights(&mut self, curves: &[RoadCurve], junctions: &[roads::junction::Junction]) {
        self.light_poses.clear();
        for sl in &self.stop_lines {
            let Some(plan) = self.plans.get(sl.junction).cloned().flatten() else {
                continue;
            };
            if plan.control == Control::Roundabout {
                continue;
            }
            let Some(j) = junctions.get(sl.junction) else { continue };
            let Some(leg) = j.legs.iter().find(|l| l.curve == sl.curve) else {
                continue;
            };
            let Some(c) = curves.get(sl.curve) else { continue };
            let (p, _yaw) = c.sample_at(sl.s).unwrap_or((Vec3::new(j.x as f32, 0.0, j.z as f32), 0.0));
            // travel direction: vehicles arrive moving OPPOSITE the leg's
            // outward dir; right of travel = perp of (out_dir flipped)
            let travel = (leg.out_dir.0, leg.out_dir.1); // outward from junction
            let back = (leg.half_width + 2.5).max(5.0);
            let right = (leg.half_width + 1.2).max(2.0);
            // unit outward
            let ol = (travel.0.hypot(travel.1)).max(1e-6);
            let (ox, oz) = (travel.0 / ol, travel.1 / ol);
            // right of OUTWARD = left of approach... approach travels -out;
            // right of approach travel = perp(-out) = (-oz, ox)... use the
            // approach's right so the pole sits on the arriving driver's right
            let (rx, rz) = (-oz, ox);
            let bearing = (-ox).atan2(-oz);
            self.light_poses.push(LightPose {
                x: j.x + ox * back + rx * right,
                z: j.z + oz * back + rz * right,
                elev: p.y as f64,
                bearing,
                junction: sl.junction,
                group: sl.group,
                positive_dir: sl.positive_dir,
            });
        }
    }

    fn rand(&mut self) -> f64 {
        // xorshift64*
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let v = x.wrapping_mul(0x2545F4914F6CDD1D);
        ((v >> 11) as f64) / ((1u64 << 53) as f64)
    }

    fn spawn_initial(&mut self, curves: &[RoadCurve]) {
        let lengths = self.lengths.clone();
        let two_way = self.two_way.clone();
        for (ci, len) in lengths.iter().enumerate() {
            if *len < 20.0 {
                continue;
            }
            // walkways / cycleways (very narrow cross-sections) carry no
            // vehicles — only the road network does
            if curves.get(ci).and_then(|c| c.points.first()).map(|p| p.lane_width < 2.2).unwrap_or(false) {
                continue;
            }
            let dirs = if two_way[ci] { 2 } else { 1 };
            for dir in 0..dirs {
                let positive = dir == 0;
                let count = ((*len / 1000.0) * self.settings.density).round() as usize;
                for _ in 0..count {
                    let s = self.rand() * len;
                    let v = self.make_vehicle(ci, s, positive);
                    self.vehicles.push(v);
                }
            }
        }
    }

    fn make_vehicle(&mut self, curve: usize, s: f64, positive: bool) -> Vehicle {
        let flip = !positive;
        // lane offset handled at draw time from the curve's cross-section;
        // store 0 here and let props() compute the real lateral
        Vehicle {
            curve,
            s,
            speed: self.settings.vmax * (0.5 + 0.5 * self.rand()),
            lateral: 0.0,
            flip,
            scale: 0.92 + 0.16 * self.rand(),
        }
    }

    /// Number of located stop lines (junction approaches).
    pub fn stop_line_count(&self) -> usize {
        self.stop_lines.len()
    }

    /// Signal state for a junction approach: true = GO (green or amber pass).
    fn is_go(&self, junction: usize, group: bool) -> bool {
        if let Some(Some(plan)) = self.plans.get(junction) {
            let g = usize::from(group);
            return plan.is_go(g, self.t);
        }
        let cycle = (self.t + self.phases.get(junction).copied().unwrap_or(0.0)) % 20.0;
        // group true: green [0,8) amber [8,10); group false: green [10,18) amber [18,20)
        if group {
            cycle < 8.0 || (cycle < 10.0)
        } else {
            (10.0..18.0).contains(&cycle) || cycle >= 18.0
        }
    }

    /// Public signal-state read for overlays: the rendered state of a
    /// junction's approach group (None = no signal at that junction).
    pub fn light_state(&self, junction: usize, group: bool) -> Option<SignalState> {
        match self.plans.get(junction) {
            Some(Some(plan)) => Some(plan.state_at(usize::from(group), self.t)),
            _ => None,
        }
    }

    /// The plan registered at a junction (None = uncontrolled).
    pub fn plan_of(&self, junction: usize) -> Option<&crate::signals::SignalPlan> {
        self.plans.get(junction).and_then(|p| p.as_ref())
    }

    /// Junction world position + kind (overlays / Signals panel).
    pub fn junction_info(&self, junction: usize) -> Option<((f64, f64), roads::junction::JunctionKind)> {
        Some((self.junction_pos.get(junction).copied()?, self.kinds.get(junction).copied()?))
    }

    pub fn junction_count(&self) -> usize {
        self.junction_pos.len()
    }

    /// Turn-passage labels for one junction's approaches (L·TH·R overlays):
    /// world position slightly upstream of each stop line + the approach
    /// cross-section summary of the nearest RoadPoint.
    pub fn approach_labels(
        &self,
        curves: &[RoadCurve],
        junction: usize,
    ) -> Vec<(f64, f64, f64, String)> {
        let mut out = Vec::new();
        for sl in &self.stop_lines {
            if sl.junction != junction {
                continue;
            }
            let Some(c) = curves.get(sl.curve) else { continue };
            let back = 9.0f64;
            let s_lbl = if sl.positive_dir { sl.s - back } else { sl.s + back };
            let Some((p, yaw)) = c.sample_at(s_lbl) else { continue };
            // right of approach travel (approach moves toward the junction:
            // +s for positive_dir stop lines, -s otherwise)
            let travel_yaw = if sl.positive_dir { yaw } else { yaw + std::f64::consts::PI };
            let right = (-travel_yaw.sin(), travel_yaw.cos());
            let half = c.paved_width() / 2.0 + 1.6;
            // nearest RoadPoint for the turn-lane summary
            let label = c
                .points
                .iter()
                .min_by(|a, b| {
                    let da = (a.x - p.x as f64).hypot(a.z - p.z as f64);
                    let db = (b.x - p.x as f64).hypot(b.z - p.z as f64);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|pt| pt.approach_summary())
                .unwrap_or_default();
            out.push((
                p.x as f64 + right.0 * half,
                p.z as f64 + right.1 * half,
                p.y as f64,
                label,
            ));
        }
        out
    }

    /// Advance the simulation `dt` seconds.
    pub fn step(&mut self, dt: f64, curves: &[RoadCurve]) {
        if dt <= 0.0 {
            return;
        }
        self.t += dt;
        // group vehicles by (curve, direction) for car following
        let n = self.vehicles.len();
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| {
            let (ca, cb) = (self.vehicles[a].curve, self.vehicles[b].curve);
            if ca != cb {
                ca.cmp(&cb)
            } else {
                let (fa, fb) = (self.vehicles[a].flip, self.vehicles[b].flip);
                if fa != fb {
                    fa.cmp(&fb)
                } else {
                    self.vehicles[a].s.partial_cmp(&self.vehicles[b].s).unwrap_or(std::cmp::Ordering::Equal)
                }
            }
        });

        let v_max = self.settings.vmax;
        let car_len = 5.0f64;
        for k in 0..n {
            let i = order[k];
            let cur = self.vehicles[i];
            let len = self.lengths.get(cur.curve).copied().unwrap_or(0.0);
            if len <= 1.0 {
                continue;
            }
            let dir_sign = if cur.flip { -1.0 } else { 1.0 };

            // car following: nearest vehicle ahead in the same (curve, dir)
            let mut v_limit = v_max;
            for &j in &order {
                if j == i {
                    continue;
                }
                let o = self.vehicles[j];
                if o.curve != cur.curve || o.flip != cur.flip {
                    continue;
                }
                // ahead = in travel direction from cur, within 40 m
                let gap = if !cur.flip {
                    let g = o.s - cur.s - car_len;
                    if o.s >= cur.s { g } else { g + len }
                } else {
                    let g = cur.s - o.s - car_len;
                    if o.s <= cur.s { g } else { g + len }
                };
                if gap < 40.0 {
                    let safe = (2.0 * gap).max(0.0).sqrt() * 1.6;
                    v_limit = v_limit.min(safe.max(0.0));
                }
            }

            // signals: stop at the nearest stop line ahead within 45 m on red
            for sl in &self.stop_lines {
                if sl.curve != cur.curve || sl.positive_dir == cur.flip {
                    continue;
                }
                let dist = if !cur.flip {
                    let d = sl.s - cur.s;
                    if d >= 0.0 { d } else { d + len }
                } else {
                    let d = cur.s - sl.s;
                    if d >= 0.0 { d } else { d + len }
                };
                if dist > 45.0 {
                    continue;
                }
                let go = self.is_go(sl.junction, sl.group);
                if !go {
                    // brake to stop 3 m before the line
                    let stop_dist = (dist - 3.0).max(0.0);
                    let v_stop = (2.0 * stop_dist * 2.2).sqrt(); // decel 2.2 m/s²
                    v_limit = v_limit.min(v_stop);
                }
            }

            // integrate speed toward v_limit
            let accel = if v_limit > cur.speed.abs() { 2.6 } else { 4.5 };
            let dv = (v_limit - cur.speed.abs()).clamp(-accel * dt, accel * dt);
            let mut new_speed = (cur.speed.abs() + dv).max(0.0);
            if new_speed < 0.15 && v_limit < 0.4 {
                new_speed = 0.0; // held at red
            }
            let sign = dir_sign;
            self.vehicles[i].speed = new_speed * sign;
            self.vehicles[i].s += new_speed * sign * dt;

            // wrap around curve ends (keeps density constant)
            let len = self.lengths[self.vehicles[i].curve];
            if self.vehicles[i].s > len {
                self.vehicles[i].s -= len;
            }
            if self.vehicles[i].s < 0.0 {
                self.vehicles[i].s += len;
            }
        }
    }

    /// Render-ready PropDraws (animated every frame by the app).
    pub fn draws(&self, curves: &[RoadCurve], library: &AssetLibrary) -> Vec<PropDraw> {
        let Some(idx) = library.index.get("vehicle").copied() else {
            return Vec::new();
        };
        self.prop_instances(curves)
            .into_iter()
            .map(|p| PropDraw {
                asset: idx,
                model: Mat4::prop_model(p.pos, p.yaw as f32, p.scale as f32),
                tex: -1,
            })
            .collect()
    }

    /// Vehicle prop draws (sampled positions + right-hand lateral offsets).
    pub fn prop_instances(&self, curves: &[RoadCurve]) -> Vec<PropInstance> {
        let mut out = Vec::with_capacity(self.vehicles.len());
        for v in &self.vehicles {
            let Some(c) = curves.get(v.curve) else { continue };
            if c.points.len() < 2 {
                continue;
            }
            let len = self.lengths.get(v.curve).copied().unwrap_or(0.0);
            if len <= 1.0 {
                continue;
            }
            let s_eff = if v.flip { len - v.s } else { v.s };
            let Some((pos, yaw)) = c.sample_at(s_eff) else { continue };
            // lane center: right of travel; reverse travel flips right.
            // left-hand networks (Tanzania) mirror the whole offset.
            let p0 = c.points.first();
            let (m_half, lw) = match p0 {
                Some(p) => (p.median / 2.0, p.lane_width.max(2.5)),
                None => (0.0, 3.5),
            };
            let side = if self.settings.left_hand { -1.0 } else { 1.0 };
            // lane 0 (innermost) center for now — could spread across lanes
            let lat = if v.flip { -(m_half + 0.5 * lw) } else { m_half + 0.5 * lw } * side;
            let (rx, rz) = (yaw.cos(), yaw.sin());
            let (px, pz) = (-rz, rx); // right of travel in (x, z)
            let y = pos.y + 0.08;
            out.push(PropInstance {
                asset: "vehicle".into(),
                pos: Vec3::new((pos.x as f64 + px * lat) as f32, y as f32, (pos.z as f64 + pz * lat) as f32),
                yaw: yaw + if v.flip { std::f64::consts::PI } else { 0.0 },
                scale: v.scale,
                label: String::new(),
                owner: "__sim".into(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roads::model::{RoadCurve, RoadPoint};

    fn straight_ew(name: &str) -> RoadCurve {
        let mut c = RoadCurve::new(name);
        for x in [-100.0, 0.0, 100.0] {
            let mut p = RoadPoint::new(x, 0.0);
            p.auto_bearing = true;
            p.lanes = 2;
            c.add_point(p);
        }
        c
    }

    #[test]
    fn vehicles_move_and_stop() {
        let a = straight_ew("A");
        let mut b = RoadCurve::new("B");
        for z in [-100.0, 0.0, 100.0] {
            let mut p = RoadPoint::new(0.0, z);
            p.auto_bearing = true;
            p.lanes = 2;
            b.add_point(p);
        }
        let curves = vec![a, b];
        let junctions = roads::junction::detect(&curves, &Default::default());
        assert_eq!(junctions.len(), 1, "crossing detected");
        let mut sim = TrafficSim::new(&curves, &junctions, SimSettings { density: 40.0, ..Default::default() });
        assert!(!sim.vehicles.is_empty(), "vehicles spawned");
        let props = sim.prop_instances(&curves);
        assert_eq!(props.len(), sim.vehicles.len());
        // step: positions change
        let before: Vec<f64> = sim.vehicles.iter().map(|v| v.s).collect();
        for _ in 0..30 {
            sim.step(0.1, &curves);
        }
        let moved = sim
            .vehicles
            .iter()
            .zip(before.iter())
            .any(|(v, &s)| (v.s - s).abs() > 0.01);
        assert!(moved, "vehicles advanced");
        // determinism: same seed → same spawn set
        let sim2 = TrafficSim::new(&curves, &junctions, SimSettings { density: 40.0, ..Default::default() });
        assert_eq!(sim2.vehicles.len(), sim.vehicles.len());
        let _ = &sim.stop_lines;
        assert!(!sim.stop_lines.is_empty(), "stop lines located at the junction");
    }
}
