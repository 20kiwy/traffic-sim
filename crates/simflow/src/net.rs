//! The directed lane graph compiled from the road network.
//!
//! Every carriageway becomes one [`Lane`] per lane index (with the
//! LHT-correct lateral offset), junction movements become connector
//! lanes (cubic-bezier polylines through the junction), and every
//! signalized approach gets a [`StopLine`] at the zebra crossing.

use crate::signals::Light;
use gdmath::vec3::Vec3;
use roads::junction::Junction;
use roads::model::{parse_turn_lanes, RoadCurve, RoadPoint, TurnDir};
use roads::tanzania;

/// Movement through a junction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Movement {
    Left,
    Through,
    Right,
}

impl Movement {
    pub fn label(self) -> &'static str {
        match self {
            Movement::Left => "L",
            Movement::Through => "TH",
            Movement::Right => "R",
        }
    }
}

/// A directed lane in the graph.
#[derive(Clone, Debug)]
pub struct Lane {
    pub id: usize,
    /// RoadCurve index (usize::MAX for connector lanes)
    pub curve: usize,
    /// true = travels +s along the curve (forward carriageway)
    pub forward: bool,
    /// lane index within the carriageway (0 = innermost / median side)
    pub lane_index: usize,
    /// signed lateral offset applied along "right of travel" (m);
    /// negative = left of travel (Tanzania LHT forward lanes)
    pub lateral: f64,
    /// arc length (m) — for road lanes the full curve, for connectors
    /// the polyline length
    pub length: f64,
    /// connector polyline (world space) — empty for road lanes
    pub polyline: Vec<Vec3>,
    /// outgoing movements: (movement, target lane id) — road lanes get
    /// up to 3 connectors at their far junction; connectors get exactly 1
    pub next: Vec<(Movement, usize)>,
    /// stop lines along this lane (one per signalized junction passed)
    pub stops: Vec<StopLine>,
}

/// A signalized stop line at the far end of a lane.
#[derive(Clone, Copy, Debug)]
pub struct StopLine {
    /// lane-local arc position of the stop line
    pub s: f64,
    /// junction index (into the signal closure)
    pub junction: usize,
    /// axis group (alternates with the cross street)
    pub group: bool,
}

/// A zebra-crossing occupancy window (pedestrian phase).
#[derive(Clone, Copy, Debug)]
pub struct PedWindow {
    pub junction: usize,
    pub from: f64,
    pub until: f64,
}

#[derive(Default)]
pub struct LaneNet {
    pub lanes: Vec<Lane>,
    /// lane ids that carry stop lines
    pub stop_lines: Vec<usize>,
    /// lane ids that are connectors
    pub connectors: Vec<usize>,
    /// connector metadata (movement) by lane id
    pub connector_move: Vec<(usize, Movement)>,
    pub junction_pos: Vec<(f64, f64)>,
}

/// Wrap an angle to (-π, π].
fn wrap(a: f64) -> f64 {
    let mut a = a;
    while a > std::f64::consts::PI {
        a -= std::f64::consts::TAU;
    }
    while a <= -std::f64::consts::PI {
        a += std::f64::consts::TAU;
    }
    a
}

fn polyline_length(pts: &[Vec3]) -> f64 {
    pts.windows(2)
        .map(|w| ((w[1].x - w[0].x).hypot(w[1].z - w[0].z)) as f64)
        .sum()
}

/// Sample a point + yaw along a connector polyline (arc s).
pub fn sample_polyline(pts: &[Vec3], s: f64) -> (Vec3, f64) {
    let mut acc = 0.0f64;
    for w in pts.windows(2) {
        let seg = ((w[1].x - w[0].x).hypot(w[1].z - w[0].z)) as f64;
        if acc + seg >= s || seg <= 0.0 {
            let t = if seg > 0.0 { (s - acc) / seg } else { 0.0 };
            let x = w[0].x + (w[1].x - w[0].x) * t as f32;
            let y = w[0].y + (w[1].y - w[0].y) * t as f32;
            let z = w[0].z + (w[1].z - w[0].z) * t as f32;
            let yaw = ((w[1].z - w[0].z) as f64).atan2((w[1].x - w[0].x) as f64);
            return (Vec3::new(x, y, z), yaw);
        }
        acc += seg;
    }
    let last = pts.last().copied().unwrap_or(Vec3::ZERO);
    (last, 0.0)
}

fn turn_moves(t: TurnDir) -> &'static [Movement] {
    match t {
        TurnDir::Left => &[Movement::Left],
        TurnDir::Through => &[Movement::Through],
        TurnDir::Right => &[Movement::Right],
        TurnDir::LeftThrough => &[Movement::Left, Movement::Through],
        TurnDir::ThroughRight => &[Movement::Through, Movement::Right],
        TurnDir::LeftRight => &[Movement::Left, Movement::Right],
        TurnDir::LeftThroughRight => &[Movement::Left, Movement::Through, Movement::Right],
        _ => &[],
    }
}

/// World pose (x, z, yaw) on a road lane at arc position `s_local`,
/// including the lane's lateral offset along right-of-travel.
pub fn lane_pose(c: &RoadCurve, lane: &Lane, s_local: f64) -> Option<(f64, f64, f64)> {
    let len = c.length_m();
    let s_curve = if lane.forward { s_local } else { len - s_local };
    let (p, yaw) = c.sample_at(s_curve)?;
    let y = if lane.forward { yaw as f64 } else { yaw as f64 + std::f64::consts::PI };
    let (px, pz) = (-y.sin(), y.cos());
    Some((p.x as f64 + px * lane.lateral, p.z as f64 + pz * lane.lateral, y))
}

impl LaneNet {
    /// Compile the lane graph from the road network + detected
    /// junctions. `left_hand` = Tanzania LHT lane sides.
    pub fn build(curves: &[RoadCurve], junctions: &[Junction], left_hand: bool) -> LaneNet {
        let mut net = LaneNet {
            junction_pos: junctions.iter().map(|j| (j.x, j.z)).collect(),
            ..Default::default()
        };
        let side = if left_hand { -1.0f64 } else { 1.0 };

        /* ---- 1. road lanes (one per marked lane per carriageway) ---- */
        for (ci, c) in curves.iter().enumerate() {
            if c.points.is_empty() {
                continue;
            }
            let p0 = &c.points[0];
            let lw = p0.lane_width.max(2.2);
            let m_half = p0.median / 2.0;
            let rev = p0.rev_lanes;
            let fwd = c.points[0].lanes.saturating_sub(rev).max(1);
            let oneway = p0.oneway || rev == 0;
            let len = c.length_m();
            if len < 4.0 || lw < 2.2 {
                continue; // walkways and slivers carry no vehicles
            }
            let mut add_lane = |net: &mut LaneNet, forward: bool, lane_index: usize| {
                // LHT: forward traffic keeps LEFT of travel; the reverse
                // carriageway sits on the other side (mirrors
                // roadkit::traffic_sim's offsets exactly).
                let inner = m_half + (lane_index as f64 + 0.5) * lw;
                let lateral = if forward { side * inner } else { -side * inner };
                let id = net.lanes.len();
                net.lanes.push(Lane {
                    id,
                    curve: ci,
                    forward,
                    lane_index,
                    lateral,
                    length: len,
                    polyline: Vec::new(),
                    next: Vec::new(),
                    stops: Vec::new(),
                });
            };
            for i in 0..fwd {
                add_lane(&mut net, true, i);
            }
            if !oneway {
                for i in 0..rev.max(1) {
                    add_lane(&mut net, false, i);
                }
            }
        }

        /* ---- 2. stop lines per junction approach ---- */
        for (ji, j) in junctions.iter().enumerate() {
            if j.kind == roads::junction::JunctionKind::Roundabout {
                continue; // circulating flow: no signals
            }
            let primary = j
                .legs
                .first()
                .map(|l| l.out_dir.1.atan2(l.out_dir.0))
                .unwrap_or(0.0);
            for leg in &j.legs {
                let Some(c) = curves.get(leg.curve) else { continue };
                let Some((s_j, _)) = c.closest_s(j.x, j.z) else { continue };
                let off = (leg.half_width + 1.0).max(3.0)
                    + tanzania::ZEBRA_WIDTH
                    + tanzania::STOP_LINE_SETBACK;
                // axis group (same convention as the existing Signals UI)
                let a = leg.out_dir.1.atan2(leg.out_dir.0);
                let diff = ((a - primary).abs().to_degrees().rem_euclid(180.0) - 90.0).abs();
                let group = diff < 45.0;
                let len = c.length_m();
                for lane in net.lanes.iter_mut().filter(|l| l.curve == leg.curve) {
                    // every travel direction through the junction stops
                    // before it (junctions may sit mid-lane)
                    let s_stop = if lane.forward {
                        (s_j - off).max(0.0)
                    } else {
                        (len - (s_j + off)).max(0.0)
                    };
                    if s_stop > 0.5 && s_stop < lane.length - 0.5 {
                        lane.stops.push(StopLine { s: s_stop, junction: ji, group });
                    }
                }
            }
        }

        /* ---- 3. connectors per junction: every arrival lane × every
        departure lane of the OTHER legs, classified by geometry ---- */
        for (ji, j) in junctions.iter().enumerate() {
            if j.kind == roads::junction::JunctionKind::Roundabout {
                continue;
            }
            // arrival + departure world poses per lane of this junction
            let mut arrivals: Vec<(f64, f64, f64, usize)> = Vec::new();
            let mut departures: Vec<(f64, f64, f64, usize)> = Vec::new();
            for lane in &net.lanes {
                if lane.curve == usize::MAX {
                    continue;
                }
                let Some(c) = curves.get(lane.curve) else { continue };
                let Some((s_j, _)) = c.closest_s(j.x, j.z) else { continue };
                let len = c.length_m();
                let s_j_local = if lane.forward { s_j } else { len - s_j };
                // every lane arrives AND departs at a mid-lane junction:
                // pose 2 m before (approach) + 6 m past (exit)
                if let Some((x, z, y)) = lane_pose(c, lane, (s_j_local - 2.0).max(0.3)) {
                    arrivals.push((x, z, y, lane.id));
                }
                if let Some((x, z, y)) = lane_pose(c, lane, (s_j_local + 6.0).min(len - 0.3)) {
                    departures.push((x, z, y, lane.id));
                }
            }
            // classify every arrival × departure pair (same curve = no
            // U-turns; through stays on the lane), keeping the
            // straightest match per movement
            for &(ax, az, ayaw, al) in &arrivals {
                let mut best: [Option<(f64, usize, f64, f64, f64)>; 3] = [None; 3];
                for &(dx, dz, dyaw, dl) in &departures {
                    if net.lanes[al].curve == net.lanes[dl].curve {
                        continue;
                    }
                    let chord = (dx - ax).hypot(dz - az);
                    if chord < 3.0 || chord > 120.0 {
                        continue;
                    }
                    let a = wrap(dyaw - ayaw);
                    let slot = if a < -0.35 {
                        0 // Left
                    } else if a > 0.35 {
                        2 // Right
                    } else {
                        1 // Through (only matches slip-through alignments)
                    };
                    let score = -a.abs();
                    if best[slot].is_none() || score > best[slot].unwrap().0 {
                        best[slot] = Some((score, dl, dx, dz, dyaw));
                    }
                }
                for (slot, b) in best.iter().enumerate() {
                    if let Some((_, dl, dx, dz, dyaw)) = b {
                        let mv = match slot {
                            0 => Movement::Left,
                            1 => Movement::Through,
                            _ => Movement::Right,
                        };
                        Self::materialize_connector(&mut net, al, mv, *dl, ax, az, ayaw, *dx, *dz, *dyaw);
                    }
                }
            }
        }

        /* ---- 4. index stop lines + connectors ---- */
        net.stop_lines = net
            .lanes
            .iter()
            .filter(|l| !l.stops.is_empty())
            .map(|l| l.id)
            .collect();
        net
    }

    /// Build one connector lane (cubic bezier polyline) from an
    /// approach lane to its departure target.
    #[allow(clippy::too_many_arguments)]
    fn materialize_connector(
        net: &mut LaneNet,
        from: usize,
        mv: Movement,
        to: usize,
        ax: f64,
        az: f64,
        ayaw: f64,
        dx: f64,
        dz: f64,
        dyaw: f64,
    ) {
        // avoid duplicate (from, mv, to) triples
        if net.lanes[from]
            .next
            .iter()
            .any(|(m, t)| (*m == mv && *t == to) || *t == to)
        {
            return;
        }
        let chord = (dx - ax).hypot(dz - az);
        if chord < 2.0 {
            return;
        }
        let h = (chord * 0.45).min(24.0);
        let p0 = Vec3::new(ax as f32, 0.0, az as f32);
        let p1 = Vec3::new((ax + ayaw.cos() * h) as f32, 0.0, (az + ayaw.sin() * h) as f32);
        let p3 = Vec3::new(dx as f32, 0.0, dz as f32);
        let p2 = Vec3::new((dx - dyaw.cos() * h) as f32, 0.0, (dz - dyaw.sin() * h) as f32);
        let mut poly = Vec::with_capacity(13);
        for k in 0..=12 {
            let t = k as f32 / 12.0;
            poly.push(bez(p0, p1, p2, p3, t));
        }
        let id = net.lanes.len();
        let length = polyline_length(&poly).max(1.0);
        net.lanes.push(Lane {
            id,
            curve: usize::MAX,
            forward: true,
            lane_index: 0,
            lateral: 0.0,
            length,
            polyline: poly,
            next: vec![(mv, to)],
            stops: Vec::new(),
        });
        net.connectors.push(id);
        net.connector_move.push((id, mv));
        net.lanes[from].next.push((mv, id));
    }

    /// The movement designations of a lane at its far junction
    /// (from the JICA/OSM turn strings); empty = unrestricted.
    pub fn lane_designations(&self, curves: &[RoadCurve], lane_id: usize) -> Vec<Movement> {
        let lane = &self.lanes[lane_id];
        let Some(c) = curves.get(lane.curve) else { return vec![] };
        let pt: &RoadPoint = if lane.forward {
            match c.points.last() {
                Some(p) => p,
                None => return vec![],
            }
        } else {
            match c.points.first() {
                Some(p) => p,
                None => return vec![],
            }
        };
        let s = if lane.forward { pt.turn_fwd.as_str() } else { pt.turn_rev.as_str() };
        if s.is_empty() {
            return vec![];
        }
        parse_turn_lanes(s)
            .iter()
            .flat_map(|t| turn_moves(*t).iter().copied())
            .collect()
    }

    /// The world pose of a lane position (road lanes sample the curve;
    /// connectors sample the polyline).
    pub fn pose(&self, curves: &[RoadCurve], lane_id: usize, s_local: f64) -> Option<(f64, f64, f64)> {
        let lane = &self.lanes[lane_id];
        if let Some(c) = curves.get(lane.curve) {
            lane_pose(c, lane, s_local)
        } else if !lane.polyline.is_empty() {
            let (p, yaw) = sample_polyline(&lane.polyline, s_local);
            Some((p.x as f64, p.z as f64, yaw as f64))
        } else {
            None
        }
    }
}

fn bez(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let u = 1.0 - t;
    Vec3::new(
        u * u * u * p0.x + 3.0 * u * u * t * p1.x + 3.0 * u * t * t * p2.x + t * t * t * p3.x,
        u * u * u * p0.y + 3.0 * u * u * t * p1.y + 3.0 * u * t * t * p2.y + t * t * t * p3.y,
        u * u * u * p0.z + 3.0 * u * u * t * p1.z + 3.0 * u * t * t * p2.z + t * t * t * p3.z,
    )
}

/// Light lookup for a lane's stop line (junction × group × time),
/// with pedestrian-phase override.
pub(crate) fn eval_stop(
    _net: &LaneNet,
    lane: &Lane,
    t: f64,
    signals: &crate::signals::SignalFn,
    peds: &[PedWindow],
) -> Option<Light> {
    let sl = lane.stops.first()?;
    let mut light = signals(sl.junction, sl.group, t);
    for p in peds {
        if p.junction == sl.junction && t >= p.from && t <= p.until {
            light = Light::Red;
        }
    }
    Some(light)
}
