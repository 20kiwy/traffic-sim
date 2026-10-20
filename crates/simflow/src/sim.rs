//! The simulation itself: vehicles on the lane graph, IDM stepping,
//! signal obedience (stop → wait → discharge) and zebra crossings.

use crate::idm::Idm;
use crate::net::{LaneNet, Movement, PedWindow};
use crate::signals::{Light, SignalFn};
use roads::model::RoadCurve;

/// Vehicle classes — the Dar es Salaam mix (daladala, DART BRT, cars,
/// HCV trucks, boda-boda).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleClass {
    Car,
    Daladala,
    BrtBus,
    Truck,
    Moto,
}

impl VehicleClass {
    /// Bumper length (m) and width (m).
    pub fn dims(self) -> (f64, f64) {
        match self {
            VehicleClass::Car => (4.5, 1.8),
            VehicleClass::Daladala => (5.5, 2.0),
            VehicleClass::BrtBus => (12.0, 2.55),
            VehicleClass::Truck => (9.0, 2.45),
            VehicleClass::Moto => (2.0, 0.8),
        }
    }

    /// Desired-speed factor over the network free-flow speed.
    pub fn speed_factor(self) -> f64 {
        match self {
            VehicleClass::Car => 1.0,
            VehicleClass::Daladala => 0.9,
            VehicleClass::BrtBus => 0.85,
            VehicleClass::Truck => 0.8,
            VehicleClass::Moto => 1.15,
        }
    }

    /// Asset id in the roadkit library (GLB prop per class).
    pub fn asset(self) -> &'static str {
        match self {
            VehicleClass::Car => "vehicle",
            VehicleClass::Daladala => "daladala",
            VehicleClass::BrtBus => "brt_bus",
            VehicleClass::Truck => "truck",
            VehicleClass::Moto => "moto",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SimSettings {
    /// network free-flow speed (m/s) — 50 km/h urban arterial
    pub vmax: f64,
    /// time scale for `step` (1.0 = real time)
    pub time_scale: f64,
    /// seed for deterministic class/spawn choice
    pub seed: u64,
}

impl Default for SimSettings {
    fn default() -> Self {
        Self { vmax: 13.9, time_scale: 1.0, seed: 42 }
    }
}

/// One vehicle agent.
#[derive(Clone, Copy, Debug)]
pub struct Vehicle {
    pub lane: usize,
    /// lane-local arc position (m)
    pub s: f64,
    /// signed speed (m/s, ≥ 0 — travel direction is the lane's)
    pub speed: f64,
    pub class: VehicleClass,
    /// desired movement at the next junction
    pub desired: Movement,
}

/// Render-ready pose (x, z, yaw radians, class, speed).
#[derive(Clone, Copy, Debug)]
pub struct VehiclePose {
    pub x: f64,
    pub z: f64,
    pub y: f64,
    pub yaw: f64,
    pub class: VehicleClass,
    pub speed: f64,
}

/// The lane-graph simulation.
pub struct Simulation {
    pub net: LaneNet,
    pub vehicles: Vec<Vehicle>,
    pub settings: SimSettings,
    pub t: f64,
    signals: SignalFn,
    peds: Vec<PedWindow>,
    idm: Idm,
    rng: u64,
}

impl Simulation {
    pub fn new(net: LaneNet, settings: SimSettings, signals: SignalFn) -> Self {
        Self {
            net,
            vehicles: Vec::new(),
            settings,
            t: 0.0,
            signals,
            peds: Vec::new(),
            idm: Idm::default(),
            rng: settings.seed.wrapping_mul(0x9E3779B97F4A7C15).max(1),
        }
    }

    pub fn set_signals(&mut self, signals: SignalFn) {
        self.signals = signals;
    }

    /// Lane-graph access (overlays / bridges).
    pub fn net(&self) -> &LaneNet {
        &self.net
    }

    /// Pedestrians occupy a junction's zebra crossings in a time window.
    pub fn set_pedestrians(&mut self, junction: usize, from: f64, until: f64) {
        self.peds.push(PedWindow { junction, from, until });
    }

    fn rand(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let v = x.wrapping_mul(0x2545F4914F6CDD1D);
        ((v >> 11) as f64) / ((1u64 << 53) as f64)
    }

    /// Spawn one vehicle on a lane at arc `s`.
    pub fn spawn_at(&mut self, lane: usize, s: f64) {
        let r = self.rand();
        let class = match (r * 100.0) as u32 {
            0..=54 => VehicleClass::Car,
            55..=74 => VehicleClass::Daladala,
            75..=84 => VehicleClass::BrtBus,
            85..=94 => VehicleClass::Truck,
            _ => VehicleClass::Moto,
        };
        let desired = match (self.rand() * 10.0) as u32 {
            0..=1 => Movement::Left,
            2..=3 => Movement::Right,
            _ => Movement::Through,
        };
        self.vehicles.push(Vehicle { lane, s, speed: 0.0, class, desired });
    }

    /// Seed the whole network from a density (vehicles / km / lane).
    pub fn spawn_density(&mut self, curves: &[RoadCurve], per_km: f64) {
        let lane_ids: Vec<usize> = self
            .net
            .lanes
            .iter()
            .filter(|l| l.curve != usize::MAX && l.length > 30.0)
            .map(|l| l.id)
            .collect();
        for id in lane_ids {
            let len = self.net.lanes[id].length;
            let n = ((len / 1000.0) * per_km).round() as usize;
            for _ in 0..n {
                let s = self.rand() * len;
                self.spawn_at(id, s);
            }
        }
        let _ = curves;
    }

    /// Leader gap + relative speed for vehicle `i` (same lane, ahead),
    /// including across the next lane when close to the end.
    fn leader(&self, i: usize) -> (f64, f64) {
        let v = &self.vehicles[i];
        let lane_len = self.net.lanes[v.lane].length;
        let (len_self, _) = v.class.dims();
        let mut best = (f64::MAX, 0.0);
        for (k, o) in self.vehicles.iter().enumerate() {
            if k == i || o.lane != v.lane {
                continue;
            }
            let gap = o.s - v.s - (len_self + o.class.dims().0) / 2.0;
            if gap > best.0 {
                continue;
            }
            if gap >= -1.0 {
                best = (gap.max(0.1), v.speed - o.speed);
            }
        }
        if best.0.is_finite() {
            return best;
        }
        // look across the far end into the next lane (connector target)
        let remaining = lane_len - v.s;
        if remaining < 30.0 {
            for (k, o) in self.vehicles.iter().enumerate() {
                if k == i {
                    continue;
                }
                let targets: Vec<usize> = self.net.lanes[v.lane].next.iter().map(|(_, t)| *t).collect();
                if !targets.contains(&o.lane) {
                    continue;
                }
                let (_, o_len) = o.class.dims();
                let gap = remaining + o.s - o_len / 2.0 - len_self / 2.0;
                if gap >= 0.0 && gap < best.0 {
                    best = (gap.max(0.1), v.speed - o.speed);
                }
            }
        }
        best
    }

    /// Distance to the stop line (if the lane has one ahead) and the
    /// light controlling it.
    fn stop_ahead(&self, v: &Vehicle) -> Option<(f64, Light)> {
        let lane = &self.net.lanes[v.lane];
        let sl = lane.stops.iter().filter(|sl| sl.s >= v.s - 1.0).min_by(|a, b| a.s.partial_cmp(&b.s).unwrap_or(std::cmp::Ordering::Equal))?;
        let dist = sl.s - v.s;
        if dist < -1.0 {
            return None; // already past
        }
        let mut light = (self.signals)(sl.junction, sl.group, self.t);
        for p in &self.peds {
            if p.junction == sl.junction && self.t >= p.from && self.t <= p.until {
                light = Light::Red;
            }
        }
        // amber dilemma zone: stop only when comfortably possible
        if light == Light::Amber && self.idm.can_stop_comfortably(v.speed, dist) {
            light = Light::Red;
        } else if light == Light::Amber {
            light = Light::Green;
        }
        Some((dist, light))
    }

    /// Advance the simulation `dt` seconds.
    pub fn step(&mut self, dt: f64) {
        let dt = dt * self.settings.time_scale;
        if dt <= 0.0 {
            return;
        }
        self.t += dt;
        // sort by lane then position so followers see the moved leader
        let mut order: Vec<usize> = (0..self.vehicles.len()).collect();
        order.sort_by(|&a, &b| {
            self.vehicles[a]
                .lane
                .cmp(&self.vehicles[b].lane)
                .then(self.vehicles[a].s.partial_cmp(&self.vehicles[b].s).unwrap_or(std::cmp::Ordering::Equal))
        });
        let n = order.len();
        for idx in (0..n).rev() {
            // front vehicles first so gaps open up realistically
            let i = order[idx];
            let v = self.vehicles[i];
            let lane_len = self.net.lanes[v.lane].length;
            if lane_len < 1.0 {
                continue;
            }
            let v0 = self.settings.vmax * v.class.speed_factor();
            let (gap, dv) = self.leader(i);
            let mut acc = self.idm.accel(v.speed, v0, gap, dv);

            // signal: virtual stopped leader at the stop line on red
            // (gap compensated so the bumper rests ~1 m from the line)
            if let Some((dist, light)) = self.stop_ahead(&self.vehicles[i]) {
                if light != Light::Green {
                    let virtual_gap = (dist - 1.0 + self.idm.s0).max(0.1);
                    let acc_stop = self.idm.accel(v.speed, v0, virtual_gap, v.speed);
                    acc = acc.min(acc_stop);
                }
            }

            // integrate
            let acc = acc.clamp(-6.0, 3.0);
            let mut new_speed = (v.speed + acc * dt).max(0.0);
            if new_speed < 0.12 && acc < 0.05 {
                new_speed = 0.0;
            }
            self.vehicles[i].speed = new_speed;
            self.vehicles[i].s += new_speed * dt;

            // lane transfer / wrap
            if self.vehicles[i].s >= lane_len {
                let lane_id = self.vehicles[i].lane;
                let nexts = self.net.lanes[lane_id].next.clone();
                let desired = self.vehicles[i].desired;
                let pick = nexts
                    .iter()
                    .find(|(m, _)| *m == desired)
                    .or_else(|| nexts.iter().find(|(m, _)| *m == Movement::Through))
                    .or_else(|| nexts.first());
                if let Some((_, target)) = pick {
                    self.vehicles[i].lane = *target;
                    self.vehicles[i].s = (self.vehicles[i].s - lane_len).min(1.0);
                } else {
                    // dead end — wrap to keep the network populated
                    self.vehicles[i].s = 0.0;
                }
            }
        }
    }

    /// Render-ready poses for all vehicles.
    pub fn poses(&self, curves: &[RoadCurve]) -> Vec<VehiclePose> {
        let mut out = Vec::with_capacity(self.vehicles.len());
        for v in &self.vehicles {
            let Some((x, z, yaw)) = self.net.pose(curves, v.lane, v.s) else { continue };
            let y = {
                // ride the road elevation when on a road lane
                let lane = &self.net.lanes[v.lane];
                if lane.curve != usize::MAX {
                    curves
                        .get(lane.curve)
                        .and_then(|c| {
                            let len = c.length_m();
                            let sc = if lane.forward { v.s } else { len - v.s };
                            c.sample_at(sc).map(|(p, _)| p.y as f64)
                        })
                        .unwrap_or(0.0)
                } else {
                    0.0
                }
            };
            out.push(VehiclePose { x, z, y: y + 0.08, yaw, class: v.class, speed: v.speed });
        }
        out
    }

    /// Queued vehicle count waiting at red across the network.
    pub fn queue_count(&self) -> usize {
        self.vehicles
            .iter()
            .filter(|v| v.speed < 0.1 && self.stop_ahead(v).map(|(_, l)| l != Light::Green).unwrap_or(false))
            .count()
    }
}
