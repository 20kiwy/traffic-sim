//! simflow — the lane-graph traffic simulation engine.
//!
//! A dedicated logic crate (user request: "make well logic algorithm
//! crate for simulation that will align well car with lanes and to
//! pass on intersection to test of well car will move and stop where
//! it should stop for crossing so to wait for green light to cross").
//!
//! Model
//! -----
//! - The road network is compiled into a directed **lane graph**
//!   ([`net::LaneNet`]): every carriageway of every [`RoadCurve`]
//!   becomes one lane per marked lane index (with the correct lateral
//!   offset — LEFT-hand traffic for Tanzania), every junction approach
//!   gets a **stop line** at the zebra crossing, and every allowed
//!   movement (left / through / right, from the JICA `turn:lanes`
//!   strings) becomes a **connector** arc through the junction.
//! - Vehicles follow IDM ([`idm`]) car-following: acceleration toward
//!   a desired speed, braking for the leader, for the red light at the
//!   stop line, and for pedestrians on the zebra crossing.
//! - Signal control is injected as a closure
//!   `Fn(junction, group, t) -> Light` so the engine stays independent
//!   of roadkit's registry; the roadkit bridge (see roadkit::flow)
//!   wires [`crate::signals::SignalRegistry`] presets (Ubungo 2-phase
//!   130 s, Tazara 4-phase G20/Y3/AR2, …) into it.
//! - Amber dilemma zone: vehicles stop on amber when they can do so
//!   comfortably (decel ≤ 3 m/s²), otherwise clear the junction.
//!
//! Vehicle classes (Dar es Salaam mix): daladala minibus, DART BRT bus,
//! sedan, truck (HCV) and boda-boda motorcycle — each with its own
//! length, width and desired speed factor.

pub mod idm;
pub mod net;
pub mod signals;
pub mod sim;

pub use net::LaneNet;
pub use signals::{Light, SignalFn};
pub use sim::{SimSettings, Simulation, VehicleClass, VehiclePose};

/// Crate version of the flow model (bumped with the workspace).
pub const FLOW_VERSION: &str = "1.0";

#[cfg(test)]
mod tests {
    use super::*;
    use roads::model::RoadPoint;

    fn straight_ew(name: &str, x0: f64, x1: f64, z: f64, lanes: usize, turn: &str) -> roads::model::RoadCurve {
        let mut c = roads::model::RoadCurve::new(name);
        for x in [x0, (x0 + x1) / 2.0, x1] {
            let mut p = RoadPoint::new(x, z);
            p.auto_bearing = true;
            p.lanes = lanes; // total = forward only (one-way here)
            p.rev_lanes = 0;
            p.oneway = true;
            p.lane_width = 3.2;
            p.turn_fwd = turn.to_string();
            c.add_point(p);
        }
        c
    }

    fn straight_ns(name: &str, z0: f64, z1: f64, x: f64, lanes: usize, turn: &str) -> roads::model::RoadCurve {
        let mut c = roads::model::RoadCurve::new(name);
        for z in [z0, (z0 + z1) / 2.0, z1] {
            let mut p = RoadPoint::new(x, z);
            p.auto_bearing = true;
            p.lanes = lanes;
            p.rev_lanes = 0;
            p.oneway = true;
            p.lane_width = 3.2;
            p.turn_fwd = turn.to_string();
            c.add_point(p);
        }
        c
    }

    /// One-way E-W road crossing a one-way N-S road, both through-only.
    fn crossing_net() -> (Vec<roads::model::RoadCurve>, Vec<roads::junction::Junction>) {
        let curves = vec![
            straight_ew("A", -100.0, 100.0, 0.0, 2, "through|through"),
            straight_ns("B", -100.0, 100.0, 0.0, 2, "through|through"),
        ];
        let junctions = roads::junction::detect(&curves, &Default::default());
        (curves, junctions)
    }

    #[test]
    fn net_has_lanes_connectors_and_stoplines() {
        let (curves, junctions) = crossing_net();
        assert_eq!(junctions.len(), 1, "one crossing detected");
        let net = LaneNet::build(&curves, &junctions, true);
        // 2 curves × 2 lanes = 4 directed road lanes (+ connectors)
        assert_eq!(net.lanes.iter().filter(|l| l.curve != usize::MAX).count(), 4);
        // 4 approaches with stop lines (E-W from both sides + N-S both)
        assert!(net.stop_lines.len() >= 4, "stop lines on every approach");
        // left/right connectors exist (through stays on the lane)
        assert!(net.connectors.len() >= 4, "turn movements wired");
    }

    #[test]
    fn car_stops_on_red_and_waits_for_green() {
        let (curves, junctions) = crossing_net();
        let net = LaneNet::build(&curves, &junctions, true);
        // permanently red for group 0
        let red: SignalFn = Box::new(|_j, _g, _t| Light::Red);
        let mut sim = Simulation::new(net, SimSettings { ..Default::default() }, red);
        // spawn one car 30 m along the E-W lane (junction at ~100)
        let lane = sim.net.lanes.iter().position(|l| l.curve == 0).unwrap();
        sim.spawn_at(lane, 30.0);
        let v0 = sim.vehicles[0].s;
        for _ in 0..400 {
            sim.step(0.1);
        }
        let v = &sim.vehicles[0];
        // car must have advanced to the stop line and WAITED there
        assert!(v.s > v0, "car moved toward the junction");
        let stop = *sim.net.lanes[lane].stops.first().expect("stop line on lane");
        let dist = (stop.s - v.s).abs();
        assert!(dist < 2.0, "car waits AT the stop line (dist {dist:.1} m)");
        assert!(v.speed < 0.05, "car is stopped for the red light");

        // now switch to permanent green — the car crosses
        let green: SignalFn = Box::new(|_j, _g, _t| Light::Green);
        sim.set_signals(green);
        let s_at_release = sim.vehicles[0].s;
        for _ in 0..200 {
            sim.step(0.1);
        }
        assert!(
            sim.vehicles[0].s > s_at_release + 10.0,
            "car discharged on green"
        );
    }

    #[test]
    fn cars_queue_behind_each_other() {
        let (curves, junctions) = crossing_net();
        let net = LaneNet::build(&curves, &junctions, true);
        let red: SignalFn = Box::new(|_j, _g, _t| Light::Red);
        let mut sim = Simulation::new(net, SimSettings { ..Default::default() }, red);
        let lane = sim.net.lanes.iter().position(|l| l.curve == 0).unwrap();
        sim.spawn_at(lane, 10.0);
        sim.spawn_at(lane, 40.0);
        for _ in 0..500 {
            sim.step(0.1);
        }
        let a = &sim.vehicles[0];
        let b = &sim.vehicles[1];
        // front car at the line, follower at a safe gap behind
        let gap = (b.s - a.s).abs();
        assert!(gap > 5.5 && gap < 40.0, "queued gap {gap:.1} m");
        assert!(a.speed < 0.05 && b.speed < 0.05, "both stopped");
    }

    #[test]
    fn turn_connector_carries_the_car_across() {
        // A: E-W one-way, B: N-S one-way, right turns allowed on A
        use net::Movement;
        let curves = vec![
            straight_ew("A", -100.0, 100.0, 0.0, 2, "through|right"),
            straight_ns("B", -100.0, 100.0, 0.0, 2, "through|through"),
        ];
        let junctions = roads::junction::detect(&curves, &Default::default());
        let net = LaneNet::build(&curves, &junctions, true);
        let right_connectors = net.connector_move.iter().filter(|(_, m)| *m == Movement::Right).count();
        assert!(right_connectors >= 1, "right-turn connector built");
        let green: SignalFn = Box::new(|_j, _g, _t| Light::Green);
        let mut sim = Simulation::new(net, SimSettings { ..Default::default() }, green);
        // spawn on the rightmost lane (index 1) of curve 0
        let lane = sim.net.lanes.iter().position(|l| l.curve == 0 && l.lane_index == 1).unwrap();
        sim.spawn_at(lane, 20.0);
        // the desired movement of this vehicle must be RIGHT to take the
        // connector — force it (the test targets connector traversal)
        sim.vehicles[0].desired = Movement::Right;
        for _ in 0..600 {
            sim.step(0.1);
        }
        // the car should have transferred onto curve 1 (via the connector)
        let on_b = sim.vehicles.iter().any(|v| sim.net.lanes[v.lane].curve == 1);
        assert!(on_b, "car passed through the junction onto the cross street");
    }

    #[test]
    fn pedestrian_zebra_blocks_the_approach() {
        let (curves, junctions) = crossing_net();
        let net = LaneNet::build(&curves, &junctions, true);
        let green: SignalFn = Box::new(|_j, _g, _t| Light::Green);
        let mut sim = Simulation::new(net, SimSettings { ..Default::default() }, green);
        let lane = sim.net.lanes.iter().position(|l| l.curve == 0).unwrap();
        sim.spawn_at(lane, 20.0);
        // pedestrians occupy the zebra on this approach for 5..15 s
        sim.set_pedestrians(0, 5.0, 15.0);
        // 15 s: the car has reached the line while pedestrians hold it
        for _ in 0..150 {
            sim.step(0.1);
        }
        assert!(sim.vehicles[0].speed < 0.1, "car yields to the zebra crossing");
        for _ in 0..200 {
            sim.step(0.1);
        }
        assert!(sim.vehicles[0].speed > 1.0, "car resumes after pedestrians clear");
    }


    #[test]
    fn lane_offsets_match_lht_side() {
        let (curves, junctions) = crossing_net();
        let net = LaneNet::build(&curves, &junctions, true);
        // Tanzania LHT: lane 0 (innermost) sits LEFT of the axis at
        // -median/2 - 0.5·lane_w … -median/2 - 1.5·lane_w
        let l0 = net.lanes.iter().find(|l| l.curve == 0 && l.lane_index == 0).unwrap();
        let l1 = net.lanes.iter().find(|l| l.curve == 0 && l.lane_index == 1).unwrap();
        assert!((l0.lateral - (-1.6)).abs() < 0.2, "inner lane at -1.6 m (LHT), got {}", l0.lateral);
        assert!((l1.lateral - (-4.8)).abs() < 0.2, "outer lane at -4.8 m (LHT), got {}", l1.lateral);
    }
}
