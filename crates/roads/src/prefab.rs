//! WFC-style junction prefab selection over the godot-road-generator
//! custom-container GLBs (3way / 4way / roundabout / splitter / ramps).
//!
//! The prefabs are hand-modeled intersections ("well laned" — their road
//! markings are part of the geometry, UV-mapped to the GRG trim sheet).
//! Like wave function collapse, each detected junction is a cell and every
//! prefab is a tile: we test all 8 dihedral orientations (4 rotations ×
//! z-mirror) of every candidate tile against the junction's leg directions,
//! score = Σ(leg → nearest arm angular error) + Σ(open arm penalty), and
//! collapse to the lowest-error tile when the mean error is within
//! tolerance. Bad fits fall back to the procedural n-gon patch.

/// One prefab tile: asset id (assets/glb manifest), canonical arm
/// directions (degrees, atan2(z, x) in the GLB file space) and the arm
/// road half-width at the tips (meters, file scale) used for scaling.
pub struct PrefabDef {
    pub id: &'static str,
    pub label: &'static str,
    /// arm directions in degrees
    pub arms_deg: &'static [f64],
    /// half road width at an arm tip (meters)
    pub half_arm: f64,
    /// roundabout tiles accept legs at ANY angle (the ring absorbs skew)
    pub roundabout: bool,
    /// only placeable via the manual override (geometry match too fuzzy)
    pub manual_only: bool,
    /// roundabout island radius at file scale (0 for non-roundabouts)
    pub island_r: f64,
}

pub const PREFABS: &[PrefabDef] = &[
    PrefabDef { id: "4way_2x2", label: "4-way dual carriageway", arms_deg: &[0.0, 90.0, 180.0, 270.0], half_arm: 13.76, roundabout: false, manual_only: false, island_r: 0.0 },
    PrefabDef { id: "4way_1x1", label: "4-way single carriageway", arms_deg: &[0.0, 90.0, 180.0, 270.0], half_arm: 6.25, roundabout: false, manual_only: false, island_r: 0.0 },
    PrefabDef { id: "3way_2x2", label: "T-junction dual", arms_deg: &[90.0, 180.0, 270.0], half_arm: 13.76, roundabout: false, manual_only: false, island_r: 0.0 },
    PrefabDef { id: "3way_1x1", label: "T-junction single", arms_deg: &[90.0, 180.0, 270.0], half_arm: 6.25, roundabout: false, manual_only: false, island_r: 0.0 },
    PrefabDef { id: "roundabout_2x2", label: "Roundabout dual", arms_deg: &[0.0, 90.0, 180.0, 270.0], half_arm: 17.3, roundabout: true, manual_only: false, island_r: 5.97 },
    PrefabDef { id: "roundabout_1x1", label: "Roundabout single", arms_deg: &[0.0, 90.0, 180.0, 270.0], half_arm: 6.98, roundabout: true, manual_only: false, island_r: 4.13 },
    PrefabDef { id: "splitter_2x2", label: "Splitter dual", arms_deg: &[90.0, 248.0, 292.0], half_arm: 7.94, roundabout: false, manual_only: true, island_r: 0.0 },
    PrefabDef { id: "splitter_1x1", label: "Splitter single", arms_deg: &[90.0, 248.0, 292.0], half_arm: 5.95, roundabout: false, manual_only: true, island_r: 0.0 },
    PrefabDef { id: "highway_onramp", label: "Highway on-ramp", arms_deg: &[0.0, 90.0], half_arm: 6.0, roundabout: false, manual_only: true, island_r: 0.0 },
    PrefabDef { id: "highway_offramp", label: "Highway off-ramp", arms_deg: &[90.0, 180.0], half_arm: 6.5, roundabout: false, manual_only: true, island_r: 0.0 },
];

/// A collapsed placement: asset id + world transform pieces.
#[derive(Clone, Debug)]
pub struct PrefabPlacement {
    pub id: String,
    /// yaw radians (world)
    pub yaw: f64,
    /// uniform scale (prefab arms → junction arm widths)
    pub scale: f64,
    /// mirror across the local XZ plane (z → -z) before rotation
    pub flip: bool,
}

fn ang_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).rem_euclid(360.0);
    if d > 180.0 { 360.0 - d } else { d }
}

/// Mean angular error of the best orientation for one prefab.
/// Returns (mean_error_deg, yaw_deg, flip).
fn score(p: &PrefabDef, leg_degs: &[f64]) -> (f64, f64, bool) {
    let mut best = (f64::INFINITY, 0.0, false);
    for flip in [false, true] {
        for rot in 0..4 {
            let yaw = rot as f64 * 90.0;
            let arm_world: Vec<f64> = p
                .arms_deg
                .iter()
                .map(|a| if flip { -a } else { *a } + yaw)
                .collect();
            let mut err = 0.0;
            for ld in leg_degs {
                let m = arm_world.iter().map(|a| ang_dist(*ld, *a)).fold(f64::INFINITY, f64::min);
                err += m;
            }
            // open-arm penalty: an arm with no leg wastes the tile
            for a in &arm_world {
                let matched = leg_degs.iter().any(|ld| ang_dist(*ld, *a) < 25.0);
                if !matched {
                    err += 40.0;
                }
            }
            if err < best.0 {
                best = (err, yaw, flip);
            }
        }
    }
    (best.0 / leg_degs.len().max(1) as f64, best.1, best.2)
}

/// Collapse a junction to the best-fitting prefab tile.
/// `leg_dirs` are the junction legs' outward unit vectors; `max_half` the
/// widest leg half-width (meters); `roundabout` when the junction is a
/// ring (roundabout tiles absorb any leg angles). `allow_manual` includes
/// the fuzzy tiles (splitter / ramps). Returns None = procedural patch.
pub fn collapse(
    leg_dirs: &[(f64, f64)],
    max_half: f64,
    roundabout: bool,
    allow_manual: bool,
    island_radius: f64,
) -> Option<PrefabPlacement> {
    if leg_dirs.is_empty() {
        return None;
    }
    let leg_degs: Vec<f64> = leg_dirs.iter().map(|d| d.1.atan2(d.0).to_degrees()).collect();

    if roundabout {
        // ring: any leg angles work; scale so the ISLAND matches the
        // junction's island radius (roundabouts are sized by their island,
        // not by the road width — otherwise 2-lane roads shrink the tile)
        let id = if island_radius > 11.0 || max_half > 9.0 {
            "roundabout_2x2"
        } else {
            "roundabout_1x1"
        };
        let def = PREFABS.iter().find(|p| p.id == id)?;
        let by_island = island_radius / def.island_r.max(1.0);
        let by_road = (max_half * 1.6) / def.half_arm;
        let scale = by_island.max(by_road).clamp(0.6, 3.0);
        return Some(PrefabPlacement {
            id: def.id.into(),
            yaw: 0.0,
            scale,
            flip: false,
        });
    }

    let n = leg_degs.len();
    let mut best: Option<(f64, f64, bool, &PrefabDef)> = None;
    for p in PREFABS {
        if p.roundabout || (p.manual_only && !allow_manual) {
            continue;
        }
        // tile arity filter: 3 legs → 3-arm tiles; 4 legs → 4-arm tiles
        if p.arms_deg.len() != n {
            continue;
        }
        let (mean, yaw, flip) = score(p, &leg_degs);
        if best.map(|(be, _, _, _)| mean < be).unwrap_or(true) {
            best = Some((mean, yaw, flip, p));
        }
    }
    let (mean, yaw, flip, p) = best?;
    if mean > 22.0 {
        return None; // no confident tile — the n-gon patch handles skewed crossings
    }
    Some(PrefabPlacement {
        id: p.id.into(),
        yaw: yaw.to_radians(),
        scale: (max_half / p.half_arm).clamp(0.12, 2.5),
        flip,
    })
}

/// Look up a prefab def by id (for the manual override UI).
pub fn by_id(id: &str) -> Option<&'static PrefabDef> {
    PREFABS.iter().find(|p| p.id == id)
}
