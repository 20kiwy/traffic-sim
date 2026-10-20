//! Intersection control registry — the engine's "every junction is
//! registered and controllable" layer (JICA TR2 §5.2/§5.3 signal plans).
//!
//! Each detected junction can carry one [`SignalPlan`]:
//! - [`Control::Uncontrolled`] — no control (default; free flow)
//! - [`Control::Signal`] — traffic signal: sequential phase groups
//!   (green → yellow → all-red per group), JICA-calibrated presets
//! - [`Control::Police`] — manual control (peak-period police, JICA
//!   reports this at Ubungo/Tazara peaks) — rendered amber-blinking
//! - [`Control::Roundabout`] — yield control, no signal heads
//!
//! The plan also drives the traffic sim: vehicles stop at the stop line
//! of every red group instead of the old fixed 20 s demo cycle.

/// Who controls the junction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Uncontrolled,
    Signal,
    Police,
    Roundabout,
}

impl Control {
    pub fn label(&self) -> &'static str {
        match self {
            Control::Uncontrolled => "no control",
            Control::Signal => "traffic signal",
            Control::Police => "police",
            Control::Roundabout => "roundabout yield",
        }
    }
}

/// One phase group's timing (green + clearance). JICA tables report
/// per-phase green / yellow (3 s) / all-red (2 s) — e.g. Tazara's
/// 120 s four-phase cycle: G20 Y3 AR2 ×4 (Table 5.3.1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupTiming {
    pub green: f64,
    pub yellow: f64,
    pub all_red: f64,
}

impl Default for GroupTiming {
    fn default() -> Self {
        Self { green: 20.0, yellow: 3.0, all_red: 2.0 }
    }
}

impl GroupTiming {
    pub fn span(&self) -> f64 {
        self.green + self.yellow + self.all_red
    }
}

/// A junction's signal plan. Groups run sequentially; the cycle length
/// is the sum of all group spans (130 s at Ubungo BRT Phase I, 120 s at
/// Tazara — JICA Figures 5.2.7 / 5.3.2).
#[derive(Clone, Debug, PartialEq)]
pub struct SignalPlan {
    pub name: String,
    pub control: Control,
    /// which group an approach belongs to (0..groups.len()); grouped by
    /// the junction's axis pair (N-S / E-W style assignment)
    pub groups: Vec<GroupTiming>,
}

impl SignalPlan {
    pub fn cycle_s(&self) -> f64 {
        self.groups.iter().map(|g| g.span()).sum()
    }

    /// Signal state of `group` at simulation time `t`.
    pub fn state_at(&self, group: usize, t: f64) -> SignalState {
        if self.control != Control::Signal || self.groups.is_empty() {
            return match self.control {
                Control::Police => {
                    // slow blink
                    if (t * 1.6) as i64 % 2 == 0 {
                        SignalState::Amber
                    } else {
                        SignalState::Off
                    }
                }
                Control::Roundabout => SignalState::Off,
                _ => SignalState::Off,
            };
        }
        let cycle = self.cycle_s();
        if cycle <= 0.0 {
            return SignalState::Off;
        }
        let mut phase_start = 0.0;
        for (i, ph) in self.groups.iter().enumerate() {
            let span = ph.span();
            if i == group % self.groups.len() {
                let tc = t.rem_euclid(cycle) - phase_start;
                if tc < 0.0 {
                    return SignalState::Red;
                }
                if tc < ph.green {
                    return SignalState::Green;
                }
                if tc < ph.green + ph.yellow {
                    return SignalState::Amber;
                }
                return SignalState::Red;
            }
            phase_start += span;
        }
        SignalState::Red
    }

    /// Convenience: is the group passable (green/amber)?
    pub fn is_go(&self, group: usize, t: f64) -> bool {
        matches!(self.state_at(group, t), SignalState::Green | SignalState::Amber)
    }
}

/// Live head state (what the 3D / 2D overlays render).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalState {
    Green,
    Amber,
    Red,
    Off,
}

impl SignalState {
    /// RGB (0..1) for overlays / light heads.
    pub fn rgb(&self) -> [f32; 3] {
        match self {
            SignalState::Green => [0.15, 0.85, 0.25],
            SignalState::Amber => [1.0, 0.72, 0.1],
            SignalState::Red => [0.95, 0.15, 0.12],
            SignalState::Off => [0.35, 0.35, 0.35],
        }
    }
}

/* ---------------- JICA Dar es Salaam presets ---------------- */

/// Ubungo/Kijazi BRT Phase I two-phase: cycle 130 s, all-red 2 s —
/// P1 (N-S Sam Nujoma / Nelson Mandela) 68 s, P2 (E-W Morogoro + BRT)
/// 36 s (JICA Figure 5.2.7 "Cycle time = 130 s, All-red = 2 s").
pub fn preset_ubungo_2phase() -> SignalPlan {
    SignalPlan {
        name: "Ubungo BRT Ph-I 2-phase (JICA)".into(),
        control: Control::Signal,
        groups: vec![
            GroupTiming { green: 68.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 36.0, yellow: 3.0, all_red: 2.0 },
        ],
    }
}

/// Ubungo optimized four-phase (JICA Table 5.2.1 required greens
/// 7/30/25/22 with Y3 AR2) — the "fix the intersection" at-grade case.
pub fn preset_ubungo_4phase() -> SignalPlan {
    SignalPlan {
        name: "Ubungo 4-phase optimized (JICA T5.2.1)".into(),
        control: Control::Signal,
        groups: vec![
            GroupTiming { green: 7.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 30.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 25.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 22.0, yellow: 3.0, all_red: 2.0 },
        ],
    }
}

/// Tazara four-phase: cycle 120 s, every group G20 Y3 AR2 (JICA
/// Figure 5.3.2 "Current Configuration (Cycle Length = 120 sec)").
pub fn preset_tazara_4phase() -> SignalPlan {
    SignalPlan {
        name: "Tazara 4-phase G20/Y3/AR2 (JICA)".into(),
        control: Control::Signal,
        groups: vec![
            GroupTiming { green: 20.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 20.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 20.0, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: 20.0, yellow: 3.0, all_red: 2.0 },
        ],
    }
}

/// Generic two-phase urban signal (Sam Nujoma × Igesa class).
pub fn preset_two_phase(cycle_s: f64) -> SignalPlan {
    let g = (cycle_s * 0.45 - 5.0).max(8.0);
    SignalPlan {
        name: format!("2-phase {:.0}s", cycle_s),
        control: Control::Signal,
        groups: vec![
            GroupTiming { green: g, yellow: 3.0, all_red: 2.0 },
            GroupTiming { green: cycle_s - 2.0 * g - 10.0, yellow: 3.0, all_red: 2.0 },
        ],
    }
}

/// Per-junction registry entry, anchored by junction WORLD POSITION so
/// plans survive scene rebuilds (junction indices shift as curves edit).
#[derive(Clone, Debug, PartialEq)]
pub struct SignalEntry {
    pub x: f64,
    pub z: f64,
    pub plan: SignalPlan,
}

impl SignalEntry {
    pub fn new(x: f64, z: f64, plan: SignalPlan) -> Self {
        Self { x, z, plan }
    }
}

/// The engine registry: every controlled junction registers here; the
/// Signals panel lists/edits these, the sim + overlays read them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SignalRegistry {
    pub entries: Vec<SignalEntry>,
}

impl SignalRegistry {
    pub fn add(&mut self, x: f64, z: f64, plan: SignalPlan) {
        // one plan per junction — replace when re-registering the spot
        if let Some(e) = self.entries.iter_mut().find(|e| (e.x - x).hypot(e.z - z) < 12.0) {
            e.plan = plan;
            e.x = x;
            e.z = z;
        } else {
            self.entries.push(SignalEntry { x, z, plan });
        }
    }

    pub fn remove_at(&mut self, x: f64, z: f64) {
        self.entries.retain(|e| (e.x - x).hypot(e.z - z) >= 12.0);
    }

    pub fn plan_at(&self, x: f64, z: f64) -> Option<&SignalPlan> {
        let mut best: Option<(&SignalEntry, f64)> = None;
        for e in &self.entries {
            let d = (e.x - x).hypot(e.z - z);
            if d < 12.0 && best.map(|(_, bd)| d < bd).unwrap_or(true) {
                best = Some((e, d));
            }
        }
        best.map(|(e, _)| &e.plan)
    }

    pub fn plan_for_junction(&self, jx: f64, jz: f64) -> Option<&SignalPlan> {
        self.plan_at(jx, jz)
    }

    /// Mutate the plan registered at a junction (Signals panel edits).
    pub fn plan_for_junction_mut(&mut self, jx: f64, jz: f64) -> Option<&mut SignalPlan> {
        let idx = self
            .entries
            .iter()
            .position(|e| (e.x - jx).hypot(e.z - jz) < 12.0)?;
        Some(&mut self.entries[idx].plan)
    }
}
