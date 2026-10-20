//! toolkit — the Rust port of pekaram/Traffic-Toolkit-DOTS for traffic-sim.
//!
//! The Unity DOTS original runs vehicle agents in an ECS with a preview
//! camera; here the same concepts map onto the traffic-sim stack:
//! - vehicle agents = `roadkit::traffic_sim::TrafficSim` (car-following,
//!   signal stop lines, per-junction phases) driven on the procedural
//!   RoadCurves (the godot-road-generator port),
//! - the preview camera = `CameraShot`/`CameraDirector` — an orbit camera
//!   path whose frames are captured headless (software rasterizer; run the
//!   egui app under `xvfb-run` for the interactive variant),
//! - screenshots = the CLI writes every shot to PNG via the `image` crate.

use gdmath::vec3::Vec3;
use gdmath::OrbitCam;
use roads::model::RoadCurve;

/// Toolkit tuning (Unity DOTS defaults, urban).
#[derive(Copy, Clone, Debug)]
pub struct ToolkitSettings {
    pub density: f64,
    pub vmax: f64,
    pub seed: u64,
    pub time_scale: f64,
}

impl Default for ToolkitSettings {
    fn default() -> Self {
        ToolkitSettings {
            density: 12.0,
            vmax: 13.5,
            seed: 42,
            time_scale: 1.0,
        }
    }
}

/// Running throughput/speed statistics (the toolkit's data capture).
#[derive(Copy, Clone, Debug, Default)]
pub struct ToolkitStats {
    pub sim_seconds: f64,
    pub vehicles: usize,
    pub avg_speed_kmh: f64,
    pub free_flow_kmh: f64,
    pub congestion_pct: f64,
}

/// The toolkit: wraps the TrafficSim and tracks the DOTS-style stats.
pub struct TrafficToolkit {
    pub sim: roadkit::traffic_sim::TrafficSim,
    pub stats: ToolkitStats,
    pub settings: ToolkitSettings,
}

impl TrafficToolkit {
    pub fn new(
        curves: &[RoadCurve],
        junctions: &[roads::junction::Junction],
        settings: ToolkitSettings,
    ) -> Self {
        let sim_settings = roadkit::traffic_sim::SimSettings {
            left_hand: true,
            density: settings.density,
            time_scale: settings.time_scale,
            vmax: settings.vmax,
            seed: settings.seed,
        };
        let sim = roadkit::traffic_sim::TrafficSim::new(curves, junctions, sim_settings);
        let n = sim.vehicles.len();
        TrafficToolkit {
            sim,
            stats: ToolkitStats {
                sim_seconds: 0.0,
                vehicles: n,
                avg_speed_kmh: 0.0,
                free_flow_kmh: settings.vmax * 3.6,
                congestion_pct: 0.0,
            },
            settings,
        }
    }

    /// advance the world
    pub fn step(&mut self, dt: f64, curves: &[RoadCurve]) {
        self.sim.step(dt, curves);
        self.stats.sim_seconds += dt;
        self.stats.vehicles = self.sim.vehicles.len();
        if !self.sim.vehicles.is_empty() {
            let avg = self
                .sim
                .vehicles
                .iter()
                .map(|v| v.speed.abs())
                .sum::<f64>()
                / self.sim.vehicles.len() as f64;
            self.stats.avg_speed_kmh = avg * 3.6;
            let ff = self.stats.free_flow_kmh.max(0.001);
            self.stats.congestion_pct = ((1.0 - self.stats.avg_speed_kmh / ff) * 100.0).clamp(0.0, 100.0);
        }
    }

    pub fn report(&self) -> String {
        format!(
            "traffic toolkit — vehicles {} · avg {:.1} km/h (free flow {:.1}) · congestion {:.0}% · sim {:.0}s",
            self.stats.vehicles,
            self.stats.avg_speed_kmh,
            self.stats.free_flow_kmh,
            self.stats.congestion_pct,
            self.stats.sim_seconds
        )
    }
}

/// One preview-camera frame: orbit params + output name.
#[derive(Clone, Debug)]
pub struct CameraShot {
    pub name: String,
    pub target: [f64; 3],
    pub dist: f64,
    pub yaw: f64,
    pub pitch: f64,
}

impl CameraShot {
    pub fn to_cam(&self) -> OrbitCam {
        OrbitCam {
            target: Vec3::new(self.target[0] as f32, self.target[1] as f32, self.target[2] as f32),
            dist: self.dist as f32,
            yaw: self.yaw as f32,
            pitch: self.pitch as f32,
        }
    }
}

/// bbox of a curve set: (min_x, min_z, max_x, max_z)
pub fn curves_bbox(curves: &[RoadCurve]) -> (f64, f64, f64, f64) {
    let mut bb = (f64::INFINITY, f64::INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in curves {
        for p in &c.points {
            bb.0 = bb.0.min(p.x);
            bb.1 = bb.1.min(p.z);
            bb.2 = bb.2.max(p.x);
            bb.3 = bb.3.max(p.z);
        }
    }
    if !bb.0.is_finite() {
        (0.0, 0.0, 100.0, 100.0)
    } else {
        bb
    }
}

/// Hero shots: a ring of orbit frames around the DENSE road area (junction
/// centroid — long trunk roads would skew the plain bbox center) — the
/// "user camera control" of the toolkit preview, captured headless.
pub fn hero_shots(curves: &[RoadCurve], count: usize, prefix: &str) -> Vec<CameraShot> {
    let bb = curves_bbox(curves);
    let mut cx = (bb.0 + bb.2) / 2.0;
    let mut cz = (bb.1 + bb.3) / 2.0;
    let mut n = 0usize;
    let mut jx = 0.0;
    let mut jz = 0.0;
    for c in curves {
        for p in &c.points {
            jx += p.x;
            jz += p.z;
            n += 1;
        }
    }
    if n > 0 {
        cx = jx / n as f64;
        cz = jz / n as f64;
    }
    let span = (bb.2 - bb.0).max(bb.3 - bb.1).max(60.0);
    let dist = (span * 0.30).clamp(220.0, 1500.0);
    let elev = 10.0;
    (0..count)
        .map(|i| {
            let a = (i as f64 / count.max(1) as f64) * std::f64::consts::TAU * 0.5 - 0.6;
            CameraShot {
                name: format!("{}_orbit{}", prefix, i + 1),
                target: [cx, elev, cz],
                dist,
                yaw: 0.55 + a,
                pitch: 0.58,
            }
        })
        .collect()
}

/// Close-up shots focused on one bridge — real-world detail pass.
///
/// `focus` = deck centroid `[x, y, z]` (y = exact deck height), `ground_y` =
/// terrain height under the span (deck_y − clearance), `axis` = along-deck
/// unit direction `[x, z]`. Produces a drone pass over the deck (markings,
/// gutters, railings), a low hover over the surface, under-deck views of
/// piers + clearance + the intersection below, a pier close-up and a wide
/// context frame. Under-deck cameras are clamped to stand ON the ground
/// under the span (Kijazi clearance is only ~5 m — no room below).
pub fn focus_shots(
    curves: &[RoadCurve],
    focus: [f64; 3],
    ground_y: f64,
    axis: [f64; 2],
    prefix: &str,
) -> Vec<CameraShot> {
    let bb = curves_bbox(curves);
    let span = (bb.2 - bb.0).max(bb.3 - bb.1).max(60.0);
    let deck_y = focus[1].max(2.0);
    let (ax, az) = (axis[0], axis[1]);
    // yaw that places the camera down-axis so the view direction runs ALONG
    // the deck: view dir = -(cy, sy) * dist -> (cy, sy) = (-ax, -az)
    let yaw_along = (-ax).atan2(-az);
    let (fx, fz) = (focus[0], focus[2]);
    let ahead = |t: f64| [fx + ax * 26.0 * t, fz + az * 26.0 * t];
    let a1 = ahead(0.75);
    let a2 = ahead(0.4);
    let a3 = ahead(0.3);
    // eye height that looks slightly UP at the deck from under the span
    let under = |target_y: f64, dist: f64, eye_y: f64| {
        ((eye_y - target_y) / dist).clamp(-0.35, 0.25)
    };
    vec![
        // ON-DECK DRONE: ~8 m over the lane, looking down the span —
        // markings, gutters, parapets and traffic all in frame
        CameraShot {
            name: format!("{}_deck", prefix),
            target: [a1[0], deck_y + 1.2, a1[1]],
            dist: 26.0,
            yaw: yaw_along + 0.15,
            pitch: 0.30,
        },
        // DECK CLOSE: low hover just over the surface
        CameraShot {
            name: format!("{}_deck_close", prefix),
            target: [a2[0], deck_y + 0.3, a2[1]],
            dist: 12.0,
            yaw: yaw_along - 0.35,
            pitch: 0.18,
        },
        // UNDER-DECK: from the ground beside the lower intersection, looking
        // slightly up at the soffit, piers and the traffic below
        CameraShot {
            name: format!("{}_below", prefix),
            target: [fx, deck_y - 0.8, fz],
            dist: 30.0,
            yaw: yaw_along + 1.35,
            pitch: under(deck_y - 0.8, 30.0, ground_y + 1.8),
        },
        // PIER: close-up from the lower intersection
        CameraShot {
            name: format!("{}_pier", prefix),
            target: [a3[0], deck_y - 1.5, a3[1]],
            dist: 18.0,
            yaw: yaw_along + 2.25,
            pitch: under(deck_y - 1.5, 18.0, ground_y + 1.4),
        },
        // WIDE context
        CameraShot {
            name: format!("{}_wide", prefix),
            target: [fx, deck_y, fz],
            dist: (span * 0.30).clamp(220.0, 1500.0),
            yaw: yaw_along + 0.8,
            pitch: 0.50,
        },
    ]
}
