//! Bridge: simflow ⇄ roadkit — compiles the DSM scene into a simflow
//! lane graph and wires the [`SignalRegistry`] presets into the engine's
//! signal closure, so the app and CLI can run the lane-accurate
//! simulation (red → queue → green → turn) alongside the legacy
//! curve-based one.

use crate::signals::{Control, SignalRegistry};
use gdmath::vec3::Vec3;
use roads::model::RoadCurve;
use roads::junction::Junction;
use simflow::{LaneNet, Simulation, SimSettings, VehiclePose};

/// A ready-to-run lane-graph simulation for a scene.
pub struct FlowSim {
    pub sim: Simulation,
    curves_idx: Vec<usize>,
}

impl FlowSim {
    /// Build from curves + junctions + the signal registry.
    /// `left_hand` should be true for Tanzania.
    pub fn build(
        curves: &[RoadCurve],
        junctions: &[Junction],
        registry: &SignalRegistry,
        settings: SimSettings,
        left_hand: bool,
    ) -> Self {
        let net = LaneNet::build(curves, junctions, left_hand);
        // signal closure: nearest registered junction (25 m tolerance)
        let entries: Vec<(f64, f64, crate::signals::SignalPlan)> = registry
            .entries
            .iter()
            .filter(|e| e.plan.control != Control::Uncontrolled)
            .map(|e| (e.x, e.z, e.plan.clone()))
            .collect();
        let jpos: Vec<(f64, f64)> = junctions.iter().map(|j| (j.x, j.z)).collect();
        let signals: simflow::SignalFn = Box::new(move |ji, group, t| {
            let Some(&(jx, jz)) = jpos.get(ji) else { return simflow::Light::Green };
            let Some((_, _, plan)) = entries
                .iter()
                .min_by(|a, b| {
                    let da = (a.0 - jx).hypot(a.1 - jz);
                    let db = (b.0 - jx).hypot(b.1 - jz);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
                .filter(|(ex, ez, _)| (ex - jx).hypot(ez - jz) < 25.0)
            else {
                return simflow::Light::Green; // uncontrolled: flow freely
            };
            match plan.state_at(usize::from(group), t) {
                crate::signals::SignalState::Green => simflow::Light::Green,
                crate::signals::SignalState::Amber => simflow::Light::Amber,
                crate::signals::SignalState::Red | crate::signals::SignalState::Off => simflow::Light::Red,
            }
        });
        let mut sim = Simulation::new(net, settings, signals);
        sim.spawn_density(curves, 10.0);
        Self { sim, curves_idx: Vec::new() }
    }

    /// Advance and return render poses (one per vehicle).
    pub fn step_poses(&mut self, dt: f64, curves: &[RoadCurve]) -> Vec<VehiclePose> {
        self.sim.step(dt);
        self.sim.poses(curves)
    }

    /// Queue length (vehicles currently held at red).
    pub fn queued(&self) -> usize {
        self.sim.queue_count()
    }

    /// The lane graph (for overlays: stop lines, connectors).
    pub fn net(&self) -> &LaneNet {
        &self.sim.net()
    }
}

/// Stop-line world positions for 2D/3D overlays (junction, x, z, elev).
pub fn stop_line_overlays(net: &LaneNet, curves: &[RoadCurve]) -> Vec<(usize, f64, f64, f64)> {
    let mut out = Vec::new();
    for lane in &net.lanes {
        for sl in &lane.stops {
            if let Some((x, z, _yaw)) = net.pose(curves, lane.id, sl.s) {
                let y = curves
                    .get(lane.curve)
                    .and_then(|c| {
                        let len = c.length_m();
                        let sc = if lane.forward { sl.s } else { len - sl.s };
                        c.sample_at(sc).map(|(p, _)| p.y as f64)
                    })
                    .unwrap_or(0.0);
                out.push((sl.junction, x, z, y));
            }
        }
    }
    out
}

/// Connector centerlines for 2D drawing (list of polylines).
pub fn connector_polylines(net: &LaneNet) -> Vec<Vec<Vec3>> {
    net.connectors
        .iter()
        .filter_map(|id| {
            let l = &net.lanes[*id];
            if l.polyline.is_empty() { None } else { Some(l.polyline.clone()) }
        })
        .collect()
}
