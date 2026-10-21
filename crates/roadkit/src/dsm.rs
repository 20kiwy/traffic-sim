//! Dar es Salaam scenario — the real network, JICA-TR2 calibrated.
//!
//! Ground geometry: compact OpenStreetMap extracts embedded at compile
//! time (crates/roadkit/assets/dsm/*.json — arterials, ramps, locals,
//! footways with layer/bridge/lanes/oneway tags).
//!
//! Engineering overlay (replaces OSM's sparse interchange mapping):
//! - **Kijazi Interchange (Ubungo)** — Morogoro × Sam Nujoma × Nelson
//!   Mandela: JICA §5.2 — dual carriageways with 1L/2TH/1R approaches,
//!   median BRT Phase I busway (cement, station 150 m west), the
//!   split-structure mixed-traffic flyover on the Sam Nujoma–Mandela
//!   axis (Fig 5.2.20: turning BRT passes BETWEEN the two decks),
//!   quadrant left-slip ramps, U-turn + Dala Dala terminal signals.
//! - **Tazara Intersection** — Nyerere × Nelson Mandela: JICA §5.3 —
//!   the Fig 5.3.10 concept design flyover: 300 m bridge (2 × 45 m PC
//!   hollow slabs + 120 m PC box girder over Nyerere) on approaches of
//!   137.303 m (W) / 175.408 m (E), grades 1.1→3.5→5→4→1.1 %, crown
//!   7 m; two 2-lane decks 17 m total (§5.3.5). At-grade level keeps
//!   Fig 5.3.11's plan: service roads, bus lanes + zebra bars on the
//!   Nyerere approaches, right-turn lanes under the flyover, BRT
//!   Phase-2 median busway, 4-phase 120 s signal (G20/Y3/AR2).
//! - **Fig 5.5.1 priority set** — Gerezani/Bandari (Kilwa widening +
//!   34 m six-lane Bandari + BRT-only flyover 260 m over Nyerere),
//!   Mwenge, Morocco, Magomeni, Chang'Ombe, Selanda Bridge and the
//!   Mandela Ring East junctions, each with JICA turn lanes + signals.
//! - **Sam Nujoma × Igesa (Sinza)** — channelized at-grade crossroad
//!   with a 2-phase signal (BRT Phase 4 corridor).
//!
//! Tanzania drives on the LEFT — the returned SimSettings flips the
//! sim's lateral lane offsets.

use crate::signals::{
    preset_tazara_4phase, preset_two_phase, preset_ubungo_2phase, SignalPlan, SignalRegistry,
};
use roads::model::{parse_turn_lanes, BridgeStyle, RoadCurve, RoadPoint, TurnDir};
use roads::surface;

/* ------------- embedded OSM extracts (compact JSON) ------------- */

const KIJAZI_OSM: &str = include_str!("../assets/dsm/kijazi.json");
const TAZARA_OSM: &str = include_str!("../assets/dsm/tazara.json");
const IGESA_OSM: &str = include_str!("../assets/dsm/igesa.json");
const UBUNGO_NORTH_OSM: &str = include_str!("../assets/dsm/ubungo_north.json");
const SINZA_OSM: &str = include_str!("../assets/dsm/sinza.json");

/* ------------- site anchors (lon/lat) ------------- */

/// Scene origin = Kijazi interchange center (Morogoro × Sam Nujoma).
/// Public: the app's HUD converts scene metres to lat/lon with this.
pub const GEO_ANCHOR: (f64, f64) = (39.2220, -6.7720);
const LON0: f64 = 39.2220;
const LAT0: f64 = -6.7720;
/// 111 320 m/deg · cos(6.81°) — metre-per-degree-longitude at DSM
const M_PER_DEG_LON: f64 = 110_573.0;
const M_PER_DEG_LAT: f64 = 110_574.0;

/// Tazara Intersection center (Nyerere × Nelson Mandela).
const TAZARA: (f64, f64) = (3261.0, 8647.0);
/// Sam Nujoma × Igesa junction (Sinza).
const IGESA_XZ: (f64, f64) = (1680.0, -1592.0);
/// Fig 5.5.1 priority sites (scene metres, same anchor as Kijazi):
/// estimated from the report map + OSM bearings.
const MWENGE: (f64, f64) = (2907.0, -1912.0); // Sam Nujoma × New Bagamoyo
const MOROCCO: (f64, f64) = (1658.0, 165.0); // Morogoro × Morocco
const MAGOMENI: (f64, f64) = (3427.0, 464.0); // Morogoro × Magomeni
const CHANGOMBE: (f64, f64) = (3870.0, 6191.0); // Nyerere × Chang'ombe
const GEREZANI: (f64, f64) = (4418.0, 6303.0); // Nyerere × Bandari (BRT flyover)
const BANDARI_PORT: (f64, f64) = (5184.0, 5418.0); // Bandari × Sokoine (port)
const SELANDA: (f64, f64) = (8735.0, 8393.0); // Mandela × Kilwa (Selanda Br)
const MANDELA_E: (f64, f64) = (6900.0, 8520.0); // Mandela Ring East link

fn ll2m(lon: f64, lat: f64) -> (f64, f64) {
    ((lon - LON0) * M_PER_DEG_LON, -(lat - LAT0) * M_PER_DEG_LAT)
}

/// Everything the app needs to adopt the DSM scenario.
pub struct DsmScene {
    pub curves: Vec<RoadCurve>,
    pub registry: SignalRegistry,
    /// (label, x, z) points of interest for the status bar / camera jumps
    pub pois: Vec<(String, f64, f64)>,
}

impl DsmScene {
    /// Tanzania drives on the left — hand this to the traffic sim.
    pub fn sim_settings(&self) -> crate::traffic_sim::SimSettings {
        crate::traffic_sim::SimSettings { left_hand: true, ..Default::default() }
    }
}

/* ------------- OSM JSON model (subset) ------------- */

#[derive(serde::Deserialize)]
struct OsmWay {
    pts: Vec<(f64, f64)>,
    hw: String,
    #[serde(default)]
    n: String,
    #[serde(default)]
    l: i8,
    #[serde(default)]
    b: u8,
    #[serde(default)]
    ln: String,
    #[serde(default, rename = "ow")]
    oneway: String,
}

/* ------------- classification ------------- */

struct Cross {
    lanes_per_dir: usize,
    lane_w: f64,
    surface: u8,
    walking: bool,
}

fn classify(hw: &str) -> Option<Cross> {
    let c = |lanes: usize, w: f64, s: u8| Cross { lanes_per_dir: lanes, lane_w: w, surface: s, walking: false };
    match hw {
        "motorway" | "trunk" | "primary" => Some(c(2, 3.5, surface::ASPHALT)),
        "motorway_link" | "trunk_link" | "primary_link" => Some(c(1, 3.5, surface::ASPHALT)),
        "secondary" => Some(c(1, 3.5, surface::ASPHALT)),
        "tertiary" => Some(c(1, 3.2, surface::ASPHALT)),
        "residential" | "unclassified" | "living_street" => Some(c(1, 3.0, surface::ASPHALT)),
        "service" => Some(c(1, 2.8, surface::ASPHALT)),
        "footway" | "path" | "pedestrian" | "steps" | "cycleway" => {
            Some(Cross { lanes_per_dir: 1, lane_w: 1.8, surface: surface::CONCRETE, walking: true })
        }
        _ => None,
    }
}

fn named_arterial(name: &str) -> bool {
    matches!(
        name,
        "Sam Nujoma Road"
            | "Morogoro Road"
            | "Nelson Mandela Road"
            | "Nyerere Road"
            | "Igesa Road"
            | "Shekilango"
            | "Shekilango Road"
            | "Sinza Mori Road"
            | "Bagamoyo Road"
            | "New Bagamoyo Road"
            | "Coca Cola Road"
    )
}

/// Build the full Dar es Salaam scenario (ground OSM + JICA overlay).
pub fn build_dsm() -> DsmScene {
    let mut curves: Vec<RoadCurve> = Vec::new();
    let mut registry = SignalRegistry::default();

    /* ---- 1. ground network from the OSM extracts ---- */
    for src in [KIJAZI_OSM, TAZARA_OSM, IGESA_OSM, UBUNGO_NORTH_OSM, SINZA_OSM] {
        let ways: Vec<OsmWay> = match serde_json::from_str(src) {
            Ok(v) => v,
            Err(_) => continue,
        };
        for w in ways {
            let Some(cross) = classify(&w.hw) else { continue };
            let pts: Vec<(f64, f64)> = w.pts.iter().map(|(lo, la)| ll2m(*lo, *la)).collect();
            if pts.len() < 2 {
                continue;
            }
            // length gate — drop mapping slivers, keep footpaths
            let mut len = 0.0f64;
            for i in 1..pts.len() {
                len += (pts[i].0 - pts[i - 1].0).hypot(pts[i].1 - pts[i - 1].1);
            }
            if len < (if cross.walking { 8.0 } else { 18.0 }) {
                continue;
            }
            // local-network gate: keep ALL arterials/links/footways, but
            // only residential/service near the three modelled sites
            let near_site = [TAZARA, IGESA_XZ, (0.0, 0.0)]
                .iter()
                .any(|(sx, sz)| pts.iter().any(|(x, z)| (x - sx).hypot(z - sz) < 420.0));
            let keep_local = matches!(w.hw.as_str(), "residential" | "service" | "unclassified" | "living_street");
            if keep_local && !near_site {
                continue;
            }
            // arterials inside an interchange core are REPLACED by the
            // JICA overlay below (OSM mapping there is sparse/pre-2021)
            let replaced = !cross.walking
                && matches!(w.hw.as_str(), "primary" | "trunk" | "motorway" | "primary_link" | "trunk_link" | "motorway_link" | "secondary")
                && [TAZARA, IGESA_XZ, (0.0, 0.0)]
                    .iter()
                    .any(|(sx, sz)| pts.iter().any(|(x, z)| (x - sx).hypot(z - sz) < 260.0))
                && !named_arterial(&w.n) // keep named segments OUTSIDE cores
                ;
            let replaced_core = !cross.walking
                && matches!(w.hw.as_str(), "primary" | "trunk" | "motorway" | "primary_link" | "trunk_link" | "motorway_link")
                && [TAZARA, (0.0, 0.0)].iter().any(|(sx, sz)| {
                    pts.iter().any(|(x, z)| (x - sx).hypot(z - sz) < 300.0)
                });
            if replaced || replaced_core {
                continue;
            }

            let mut curve = RoadCurve::new(if w.n.is_empty() {
                match w.hw.as_str() {
                    "footway" | "path" | "pedestrian" | "steps" => "walkway".into(),
                    "cycleway" => "cycleway".into(),
                    h if h.ends_with("_link") => "ramp".into(),
                    _ => "street".into(),
                }
            } else {
                w.n.clone()
            });
            curve.color = if cross.walking {
                [0x9a, 0x9a, 0x92]
            } else if named_arterial(&w.n) {
                [0xc8, 0x8a, 0x2a]
            } else {
                [0x70, 0x78, 0x68]
            };
            let oneway = w.oneway == "yes" || w.hw.ends_with("_link");
            let n_pts = pts.len();
            for (i, (x, z)) in pts.iter().enumerate() {
                let mut p = RoadPoint::new(*x, *z);
                p.name = format!("{}_{}", short_tag(&curve.name), i + 1);
                p.auto_bearing = true;
                p.lanes = if oneway { cross.lanes_per_dir.max(1) } else { cross.lanes_per_dir * 2 };
                p.rev_lanes = if oneway { 0 } else { cross.lanes_per_dir };
                p.oneway = oneway;
                p.lane_width = cross.lane_w;
                p.surface = cross.surface;
                p.layer = w.l;
                p.bridge = w.b == 1;
                if p.bridge && p.elev.abs() < 0.1 {
                    p.elev = 5.5; // typical urban under-bridge clearance
                }
                // walking layer: pathway-only, no markings (1 lane does that)
                if cross.walking {
                    p.lane_width = 1.8;
                    p.flatten_terrain = false;
                }
                // sidewalks on arterials (JICA cross-sections: 3 m walks)
                if !cross.walking && named_arterial(&w.n) {
                    p.pathway_l = if i == 0 || i == n_pts - 1 { p.pathway_l.max(3.0) } else { p.pathway_l };
                    p.pathway_r = p.pathway_r.max(3.0);
                }
                curve.add_point(p);
            }
            curves.push(curve);
        }
    }

    /* ---- 2. JICA overlay: Kijazi (Ubungo) ---- */
    kijazi_overlay(&mut curves, &mut registry);
    /* ---- 3. JICA overlay: Tazara ---- */
    tazara_overlay(&mut curves, &mut registry);
    /* ---- 4. JICA overlay: Sam Nujoma × Igesa ---- */
    igesa_overlay(&mut curves, &mut registry);
    /* ---- 5. Fig 5.5.1 priority corridors + sites ---- */
    priority_overlay(&mut curves, &mut registry);

    let pois = vec![
        ("Kijazi Interchange (Ubungo)".into(), 0.0, 0.0),
        ("Ubungo BRT station (150 m W)".into(), -150.0, 0.0),
        ("Tazara Intersection".into(), TAZARA.0, TAZARA.1),
        ("Sam Nujoma × Igesa (Sinza)".into(), IGESA_XZ.0, IGESA_XZ.1),
        ("Mwenge Intersection (Fig 5.5.1)".into(), MWENGE.0, MWENGE.1),
        ("Morocco Intersection".into(), MOROCCO.0, MOROCCO.1),
        ("Magomeni Intersection".into(), MAGOMENI.0, MAGOMENI.1),
        ("Chang'Ombe Intersection".into(), CHANGOMBE.0, CHANGOMBE.1),
        ("Gerezani / Bandari (BRT flyover)".into(), GEREZANI.0, GEREZANI.1),
        ("Bandari Port junction".into(), BANDARI_PORT.0, BANDARI_PORT.1),
        ("Selanda Bridge (Mandela × Kilwa)".into(), SELANDA.0, SELANDA.1),
    ];

    // R11 RoadArchitect-style guardrails on approaches/slips/BRT edges
    apply_guardrails(&mut curves);
    // R12 RA bridge kit styles (suspension / semi-arch / tunnel flags)
    apply_bridge_styles(&mut curves);

    // orient every auto_bearing point (the builders set the flag but the
    // GRG-style re-orientation normally runs from the script bridge only —
    // without this the N-S carriageways keep bearing 0 and BOW east)
    for c in &mut curves {
        c.auto_orient();
    }

    DsmScene { curves, registry, pois }
}

fn short_tag(name: &str) -> String {
    name.split_whitespace()
        .map(|w| w.chars().next().unwrap_or('?').to_ascii_uppercase().to_string())
        .collect::<Vec<_>>()
        .join("")
        .chars()
        .take(6)
        .collect()
}

/* ================= builders ================= */

/// One-way dual-carriageway pair along a straight axis (LEFT-hand
/// traffic: the A→B carriageway keeps LEFT of the A→B direction).
/// `offset` = distance from the road axis to each carriageway centerline
/// (the two carriageways end up 2·offset apart, median in between).
/// Turn strings attach per carriageway at the end near each named end:
/// `turn_near_a = (A-string, B-string)` — A travels A→B so it DEPARTS
/// the A end (through-only), B travels B→A so it ARRIVES at the A end
/// (the junction approach: "left|through|through|right").
#[allow(clippy::too_many_arguments)]
fn dual(
    curves: &mut Vec<RoadCurve>,
    name: &str,
    ax: f64,
    az: f64,
    bx: f64,
    bz: f64,
    offset: f64,
    lanes: usize,
    turn_near_a: Option<(&str, &str)>,
    turn_near_b: Option<(&str, &str)>,
    color: [u8; 3],
) {
    // unit + LEFT of travel A→B (x east, z south: left = (uz, -ux))
    let dx = bx - ax;
    let dz = bz - az;
    let l = dx.hypot(dz).max(1e-6);
    let (ux, uz) = (dx / l, dz / l);
    let (px, pz) = (uz, -ux);
    for side in [1.0, -1.0] {
        let mut c = RoadCurve::new(format!("{name} {}", if side > 0.0 { "A" } else { "B" }));
        c.color = color;
        let s = offset * side;
        let pts: Vec<(f64, f64)> = if side > 0.0 {
            // carriageway A: travels A→B on the left of that direction
            vec![(ax + px * s, az + pz * s), (bx + px * s, bz + pz * s)]
        } else {
            // carriageway B: travels B→A on the left of THAT direction
            vec![(bx - px * s, bz - pz * s), (ax - px * s, az - pz * s)]
        };
        let is_a = side > 0.0;
        for (i, (x, z)) in pts.iter().enumerate() {
            let mut p = RoadPoint::new(*x, *z);
            p.name = format!("{}_{}", short_tag(name), i + 1);
            p.auto_bearing = true;
            p.lanes = lanes;
            p.rev_lanes = 0;
            p.oneway = true;
            p.lane_width = 3.5;
            p.median = 0.0;
            p.shoulder_l = 0.5;
            p.shoulder_r = 0.5;
            p.pathway_l = 3.0;
            p.pathway_r = 3.0;
            // (A-string, B-string) at the end near this point
            let t = if i == 0 { turn_near_a } else { turn_near_b };
            if let Some((a_str, b_str)) = t {
                p.turn_fwd = if is_a { a_str.to_string() } else { b_str.to_string() };
            }
            c.add_point(p);
        }
        curves.push(c);
    }
}

/// 4-quadrant left-turn slip ramps (JICA Fig 5.2.2 / 5.3.2 — "left-turn
/// slip-ramps exist in all four quadrants"). Arc from leg A to leg B
/// around the junction center at radius r.
fn slip(curves: &mut Vec<RoadCurve>, name: &str, cx: f64, cz: f64, ang_from: f64, ang_to: f64, r: f64) {
    let mut c = RoadCurve::new(name.to_string());
    c.color = [0x9c, 0x7b, 0x3c];
    let steps = 4;
    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        let a = ang_from + (ang_to - ang_from) * t;
        let mut p = RoadPoint::new(cx + a.cos() * r, cz + a.sin() * r);
        p.name = format!("{}_{}", short_tag(name), i + 1);
        p.auto_bearing = true;
        p.lanes = 1;
        p.rev_lanes = 0;
        p.oneway = true;
        p.lane_width = 3.2;
        p.terminated = i == 0 || i == steps;
        c.add_point(p);
    }
    curves.push(c);
}

/// Flyover deck on the axis A→B: oneway structure with a 4 % approach
/// grade up to a flat crown (JICA Tazara profile, Fig 5.3.10).
#[allow(clippy::too_many_arguments)]
fn flyover(
    curves: &mut Vec<RoadCurve>,
    name: &str,
    ax: f64,
    az: f64,
    bx: f64,
    bz: f64,
    lateral: f64,
    lanes: usize,
    crown: f64,
    nose_a: f64, // fraction of span where the deck leaves ground (A side)
    nose_b: f64,
    color: [u8; 3],
) {
    let dx = bx - ax;
    let dz = bz - az;
    let l = dx.hypot(dz).max(1e-6);
    let (ux, uz) = (dx / l, dz / l);
    let (px, pz) = (-uz, ux);
    let cx = (ax + bx) / 2.0 + px * lateral;
    let cz = (az + bz) / 2.0 + pz * lateral;
    let half = l / 2.0;
    // station list: nose → crown start → crown end → nose
    let sa = -half;
    let sb = half;
    let ca = -half + (half * 2.0) * nose_a;
    let cb = half - (half * 2.0) * nose_b;
    let stations = [(sa, 0.0f64), (ca, crown), (cb, crown), (sb, 0.0)];
    let mut c = RoadCurve::new(name.to_string());
    c.color = color;
    c.underside_thickness = 1.1;
    for (i, (s, e)) in stations.iter().enumerate() {
        let mut p = RoadPoint::new(cx + ux * s, cz + uz * s);
        p.name = format!("{}_{}", short_tag(name), i + 1);
        p.auto_bearing = true;
        p.lanes = lanes;
        p.rev_lanes = 0;
        p.oneway = true;
        p.lane_width = 3.5;
        p.shoulder_l = 0.6;
        p.shoulder_r = 0.6;
        p.layer = 1;
        p.bridge = true;
        p.elev = *e;
        p.terminated = i == 0 || i == stations.len() - 1;
        c.add_point(p);
    }
    curves.push(c);
}

/// Median BRT busway (Phase I Morogoro — JICA §5.2.2: median alignment,
/// passing lanes at the station flare, cement corridor).
fn brt_median(curves: &mut Vec<RoadCurve>, name: &str, ax: f64, az: f64, bx: f64, bz: f64, station_s: Option<f64>) {
    let dx = bx - ax;
    let dz = bz - az;
    let l = dx.hypot(dz).max(1e-6);
    let (ux, uz) = (dx / l, dz / l);
    let mut c = RoadCurve::new(name.to_string());
    c.color = [0x2a, 0x9a, 0x8a];
    let mut stations: Vec<(f64, f64, f64)> = Vec::new(); // (s, elev, median)
    let n = 6;
    for i in 0..=n {
        let s = -l / 2.0 + l * (i as f64) / n as f64;
        let mut median = 1.5;
        if let Some(ss) = station_s {
            if (s - ss).abs() < 22.0 {
                median = 4.0; // station island + passing flare
            }
        }
        stations.push((s, 0.0, median));
    }
    for (i, (s, e, med)) in stations.iter().enumerate() {
        let mut p = RoadPoint::new(ax + ux * *s, az + uz * *s);
        p.name = format!("{}_{}", short_tag(name), i + 1);
        p.auto_bearing = true;
        p.lanes = 2;
        p.rev_lanes = 1;
        p.lane_width = 3.25;
        p.median = *med;
        p.elev = *e;
        p.surface = surface::CEMENT;
        p.gutter_profile = (0.0, 0.0); // raised slab, no V-gutter
        p.terminated = i == 0 || i == stations.len() - 1;
        c.add_point(p);
    }
    curves.push(c);
}

/* ---- Kijazi overlay (JICA §5.2) ---- */

fn kijazi_overlay(curves: &mut Vec<RoadCurve>, registry: &mut SignalRegistry) {
    // Morogoro Road E-W dual, ONE corridor: EB keeps left (north), WB
    // keeps left of its direction (south); 4 approach lanes per
    // direction (JICA: 1L + 2TH + 1R) at the junction ends.
    dual(
        curves,
        "Morogoro Road",
        -460.0,
        0.0,
        460.0,
        0.0,
        9.0,
        4,
        Some(("through|through|through|through", "left|through|through|right")),
        Some(("left|through|through|right", "through|through|through|through")),
        [0xc8, 0x8a, 0x2a],
    );
    // BRT Phase I median busway + station 150 m west (JICA §5.2.2)
    brt_median(curves, "BRT Ph-1 Morogoro median", -430.0, 0.0, 430.0, 0.0, Some(-150.0));
    // Nelson Mandela Road S dual: SB keeps left (east), NB west
    dual(
        curves,
        "Nelson Mandela Road",
        0.0,
        420.0,
        0.0,
        0.0,
        9.0,
        4,
        Some(("through|through|through|through", "left|through|through|right")),
        Some(("through|through|through|through", "left|through|through|right")),
        [0xc8, 0x8a, 0x2a],
    );
    // Sam Nujoma Road N dual along the real (NE) axis
    let ax = 0.649_f64;
    let az = -0.760_f64;
    dual(
        curves,
        "Sam Nujoma Road",
        0.0,
        0.0,
        ax * 420.0,
        az * 420.0,
        9.0,
        4,
        Some(("through|through|through|through", "left|through|through|right")),
        None,
        [0xc8, 0x8a, 0x2a],
    );
    // Split-structure mixed-traffic flyover (JICA Fig 5.2.20): two 2-lane
    // decks straddling the median axis; turning BRT passes BETWEEN them.
    let (px, pz) = (0.760_f64, 0.649_f64); // perp of the axis
    flyover(
        curves,
        "Kijazi flyover N (NE-bound)",
        ax * 250.0 + px * 7.5,
        az * 250.0 + pz * 7.5,
        -ax * 250.0 + px * 7.5,
        -az * 250.0 + pz * 7.5,
        0.0,
        2,
        6.5,
        0.12,
        0.12,
        [0xb0, 0x94, 0x58],
    );
    flyover(
        curves,
        "Kijazi flyover S (SW-bound)",
        ax * 250.0 - px * 7.5,
        az * 250.0 - pz * 7.5,
        -ax * 250.0 - px * 7.5,
        -az * 250.0 - pz * 7.5,
        0.0,
        2,
        6.5,
        0.12,
        0.12,
        [0xb0, 0x94, 0x58],
    );
    // Future Phase-4 BRT median between the decks (turns at grade)
    brt_median(curves, "BRT Ph-4 Sam Nujoma median (future)", ax * 180.0, az * 180.0, -ax * 180.0, -az * 180.0, None);
    // Left-turn slips, 4 quadrants (leg angles: E 0, NE -0.86, S pi/2, W pi)
    let ne = az.atan2(ax); // ≈ -0.86
    slip(curves, "Kijazi slip NE-E", 0.0, 0.0, ne, -0.28, 30.0);
    slip(curves, "Kijazi slip E-S", 0.0, 0.0, 0.28, 1.29, 30.0);
    slip(curves, "Kijazi slip S-W", 0.0, 0.0, 1.85, 2.86, 30.0);
    slip(curves, "Kijazi slip W-NE", 0.0, 0.0, -2.86, ne - 0.28, 30.0);
    // Signals (JICA Fig 5.2.7): main 2-phase 130 s + U-turn signals
    registry.add(0.0, 0.0, preset_ubungo_2phase());
    registry.add(ax * 150.0, az * 150.0, preset_two_phase(100.0));
    registry.add(0.0, 170.0, preset_two_phase(100.0));
    registry.add(0.0, 105.0, preset_two_phase(90.0)); // Dala Dala terminal
}

/* ---- Tazara overlay (JICA §5.3, Fig 5.3.10 + 5.3.11) ---- */

/// JICA Fig 5.3.10 concept-design flyover: full alignment with
/// approaches and the span-driven elevation profile (bridge 300 m =
/// 2 × 45 m hollow slabs + 120 m box girder over Nyerere; approaches
/// 137.303 m W / 175.408 m E; grades 1.1→3.5→5→4→1.1 %; crown 7 m).
#[allow(clippy::too_many_arguments)]
fn flyover_jica(
    curves: &mut Vec<RoadCurve>,
    name: &str,
    ax: f64,
    az: f64,
    bx: f64,
    bz: f64,
    lateral: f64,
    lanes: usize,
    crown: f64,
    color: [u8; 3],
) {
    let dx = bx - ax;
    let dz = bz - az;
    let l = dx.hypot(dz).max(1e-6);
    let (ux, uz) = (dx / l, dz / l);
    let (px, pz) = (-uz, ux);
    let cx = (ax + bx) / 2.0 + px * lateral;
    let cz = (az + bz) / 2.0 + pz * lateral;
    let half = l / 2.0;
    let k = crown / 7.0; // profile scale (Tazara concept is 7 m)
    // (station along the axis, elevation, on-bridge) — Fig 5.3.10 profile
    let stations: [(f64, f64, bool); 13] = [
        (-half, 0.0, false),                    // W nose (at grade)
        (-half + 40.0, 0.45 * k, false),        // 1.1 % approach
        (-half + 80.0, 0.9 * k, false),
        (-half + 120.0, 1.45 * k, false),       // 3.5 % to bridge start
        (-137.303, 1.5 * k, false),             // bridge start elevation
        (-90.0, 2.9 * k, true),                 // PC hollow slab span 1
        (-45.0, 4.3 * k, true),                 // hollow slab span 2
        (0.0, crown, true),                     // crown — box girder over Nyerere
        (45.0, 4.3 * k, true),
        (90.0, 2.9 * k, true),
        (137.303, 1.5 * k, true),               // hollow slabs east
        (half - 40.0, 0.75 * k, false),         // 4 % → 1.1 % exit
        (half, 0.0, false),                     // E nose
    ];
    let mut c = RoadCurve::new(name.to_string());
    c.color = color;
    c.underside_thickness = 1.1;
    for (i, (s, e, bridge)) in stations.iter().enumerate() {
        let mut p = RoadPoint::new(cx + ux * s, cz + uz * s);
        p.name = format!("{}_{}", short_tag(name), i + 1);
        p.auto_bearing = true;
        p.lanes = lanes;
        p.rev_lanes = 0;
        p.oneway = true;
        p.lane_width = roads::tanzania::LANE_RAMP;
        p.shoulder_l = 0.6;
        p.shoulder_r = 0.6;
        p.layer = if *bridge { 1 } else { 0 };
        p.bridge = *bridge;
        p.elev = *e;
        p.terminated = i == 0 || i == stations.len() - 1;
        c.add_point(p);
    }
    curves.push(c);
}

/// Service road (JICA at-grade plan, Fig 5.3.11: "inclusion of service
/// roads for additional capacity and land use access") — one lane each
/// way pair parallel to the arterial, kept clear of the flyover bays.
#[allow(clippy::too_many_arguments)]
fn service_road(
    curves: &mut Vec<RoadCurve>,
    name: &str,
    ax: f64,
    az: f64,
    bx: f64,
    bz: f64,
    offset: f64,
    color: [u8; 3],
) {
    let before = curves.len();
    dual(curves, name, ax, az, bx, bz, offset, 1, None, None, color);
    for c in curves.iter_mut().skip(before) {
        for p in &mut c.points {
            p.lane_width = roads::tanzania::LANE_LOCAL;
            p.pathway_l = 2.0;
            p.pathway_r = 2.0;
        }
    }
}

/// Multi-point dual carriageway corridor (left-hand traffic): the A
/// carriageway runs left of the forward direction, B left of the
/// reverse. Each vertex carries optional turn strings
/// `(a_str, b_str)`: a_str = A's FORWARD lanes at that point, b_str =
/// B's (reverse) approach lanes there — e.g. at a crossing vertex
/// `Some(("through|through", "left|through|right"))`.
#[allow(clippy::too_many_arguments)]
fn corridor_dual(
    curves: &mut Vec<RoadCurve>,
    name: &str,
    pts: &[(f64, f64, Option<&str>, Option<&str>)],
    offset: f64,
    lanes: usize,
    color: [u8; 3],
) {
    if pts.len() < 2 {
        return;
    }
    // subdivide first (see below), then per-vertex left normal from the
    // local chord of the DENSE polyline
    let mut dense0: Vec<(f64, f64, Option<&str>, Option<&str>)> = Vec::new();
    for (i, v) in pts.iter().enumerate() {
        if i > 0 {
            let pv = pts[i - 1];
            let d = (v.0 - pv.0).hypot(v.1 - pv.1);
            let n = (d / 420.0).ceil() as usize;
            for k in 1..n {
                let t = k as f64 / n as f64;
                dense0.push((pv.0 + (v.0 - pv.0) * t, pv.1 + (v.1 - pv.1) * t, None, None));
            }
        }
        dense0.push(*v);
    }
    let pts: &[(f64, f64, Option<&str>, Option<&str>)] = &dense0;
    let mut lefts: Vec<(f64, f64)> = Vec::new();
    for i in 0..pts.len() {
        let (px, pz) = if i == 0 {
            (pts[1].0 - pts[0].0, pts[1].1 - pts[0].1)
        } else if i + 1 == pts.len() {
            (pts[i].0 - pts[i - 1].0, pts[i].1 - pts[i - 1].1)
        } else {
            (pts[i + 1].0 - pts[i - 1].0, pts[i + 1].1 - pts[i - 1].1)
        };
        let l = px.hypot(pz).max(1e-6);
        // left of travel (x east, z south): left = (uz, -ux)
        lefts.push((pz / l, -px / l));
    }
    for side in [1.0, -1.0] {
        let mut c = RoadCurve::new(format!("{name} {}", if side > 0.0 { "A" } else { "B" }));
        c.color = color;
        let is_a = side > 0.0;
        let ordered: Vec<usize> = if is_a {
            (0..pts.len()).collect()
        } else {
            (0..pts.len()).rev().collect()
        };
        for (k, &pi) in ordered.iter().enumerate() {
            let (x, z, a_str, b_str) = pts[pi];
            let (lx, lz) = lefts[pi];
            let mut p = RoadPoint::new(x + lx * offset * side, z + lz * offset * side);
            p.name = format!("{}_{}", short_tag(name), k + 1);
            p.auto_bearing = true;
            p.lanes = lanes;
            p.rev_lanes = 0;
            p.oneway = true;
            p.lane_width = roads::tanzania::LANE_JICA_SIXLANE;
            p.median = 0.0;
            p.shoulder_l = roads::tanzania::SHOULDER_ARTERIAL;
            p.shoulder_r = roads::tanzania::SHOULDER_ARTERIAL;
            p.pathway_l = roads::tanzania::WALK_ARTERIAL;
            p.pathway_r = roads::tanzania::WALK_ARTERIAL;
            let t = if is_a { a_str } else { b_str };
            if let Some(s) = t {
                p.turn_fwd = s.to_string();
            }
            // interior vertices of long corridors sit mid-block
            p.terminated = k == 0 || k == ordered.len() - 1;
            c.add_point(p);
        }
        curves.push(c);
    }
}

fn tazara_overlay(curves: &mut Vec<RoadCurve>, registry: &mut SignalRegistry) {
    let (tx, tz) = TAZARA;
    // Nyerere Road E-W dual (4 approach lanes/dir: 1L 2TH 1R) with BRT
    // Phase-2 median busway (Fig 5.3.11 bus lanes on both approaches)
    dual(
        curves,
        "Nyerere Road",
        tx - 430.0,
        tz,
        tx + 430.0,
        tz,
        9.0,
        4,
        Some(("through|through|through|through", "left|through|through|right")),
        Some(("left|through|through|right", "through|through|through|through")),
        [0xc8, 0x8a, 0x2a],
    );
    brt_median(curves, "BRT Ph-2 Nyerere median", tx - 350.0, tz, tx + 350.0, tz, None);
    // Nelson Mandela Road N-S dual (through the underpass level)
    dual(
        curves,
        "Nelson Mandela Road",
        tx,
        tz - 430.0,
        tx,
        tz + 430.0,
        9.0,
        4,
        Some(("through|through|through|through", "left|through|through|right")),
        Some(("left|through|through|right", "through|through|through|through")),
        [0xc8, 0x8a, 0x2a],
    );
    // THE FLYOVER — Fig 5.3.10 concept design: Nelson Mandela over
    // Nyerere. Two 2-lane decks ±9 m off the axis (17 m total width),
    // 300 m bridge + 137.3/175.4 m approaches, crown 7 m.
    flyover_jica(
        curves,
        "Tazara flyover N (NB)",
        tx - 9.0,
        tz - 306.0,
        tx - 9.0,
        tz + 306.0,
        0.0,
        roads::tanzania::TAZARA_DECK_LANES,
        roads::tanzania::FLYOVER_CROWN,
        [0xb0, 0x94, 0x58],
    );
    flyover_jica(
        curves,
        "Tazara flyover S (SB)",
        tx + 9.0,
        tz - 306.0,
        tx + 9.0,
        tz + 306.0,
        0.0,
        roads::tanzania::TAZARA_DECK_LANES,
        roads::tanzania::FLYOVER_CROWN,
        [0xb0, 0x94, 0x58],
    );
    // Service roads in the four quadrants (Fig 5.3.11 at-grade plan)
    service_road(curves, "Tazara service W", tx - 26.0, tz - 300.0, tx - 26.0, tz + 300.0, 5.0, [0x6a, 0x70, 0x62]);
    service_road(curves, "Tazara service E", tx + 26.0, tz - 300.0, tx + 26.0, tz + 300.0, 5.0, [0x6a, 0x70, 0x62]);
    // 4-quadrant left slips (Fig 5.3.2)
    slip(curves, "Tazara slip NE", tx, tz, -1.35, -0.35, 30.0);
    slip(curves, "Tazara slip ES", tx, tz, 0.35, 1.22, 30.0);
    slip(curves, "Tazara slip SW", tx, tz, 1.92, 2.79, 30.0);
    slip(curves, "Tazara slip WN", tx, tz, -2.79, -1.79, 30.0);
    // Signal: 120 s 4-phase G20 Y3 AR2 (Fig 5.3.2)
    registry.add(tx, tz, preset_tazara_4phase());
}

/* ---- Fig 5.5.1 priority corridors + sites ---- */

/// All Fig 5.5.1 "Priority Arterial Intersections Targeted for
/// Upgrading" sites, connected by their real arterial corridors with
/// JICA turn lanes at every junction.
fn priority_overlay(curves: &mut Vec<RoadCurve>, registry: &mut SignalRegistry) {
    let app = roads::tanzania::APPROACH_1L2TH1R;
    let thru = "through|through|through|through";
    let amber = [0xc8, 0x8a, 0x2a];

    /* Morogoro Road: Kijazi → Morocco → Magomeni (E corridor) — starts
    just outside the Kijazi overlay core (which runs ±460 m) */
    corridor_dual(
        curves,
        "Morogoro East corridor",
        &[
            (470.0, 33.0, Some(thru), Some(app)),
            (MOROCCO.0, MOROCCO.1, Some(app), Some(app)),
            (MAGOMENI.0, MAGOMENI.1, Some(app), Some(app)),
            (4400.0, 700.0, Some(app), Some(thru)),
        ],
        9.0,
        3,
        amber,
    );
    registry.add(MOROCCO.0, MOROCCO.1, preset_two_phase(110.0));
    registry.add(MAGOMENI.0, MAGOMENI.1, preset_two_phase(110.0));

    /* Sam Nujoma Road: Kijazi → Mwenge (NE corridor; the report's
    "next flyover" candidate — kept at-grade with full turn lanes;
    starts outside the Kijazi overlay axis end at (272, -320)) */
    corridor_dual(
        curves,
        "Sam Nujoma North corridor",
        &[
            (300.0, -350.0, Some(thru), Some(app)),
            (1450.0, -950.0, None, None),
            (MWENGE.0, MWENGE.1, Some(app), Some(app)),
            (3700.0, -2450.0, Some(app), Some(thru)),
        ],
        9.0,
        3,
        amber,
    );
    // New Bagamoyo Road crosses at Mwenge (programmed widening)
    corridor_dual(
        curves,
        "New Bagamoyo Road",
        &[
            (MWENGE.0 - 900.0, MWENGE.1 + 1500.0, Some(thru), Some(app)),
            (MWENGE.0, MWENGE.1, Some(app), Some(app)),
            (MWENGE.0 + 1500.0, MWENGE.1 + 2600.0, Some(app), Some(thru)),
        ],
        8.0,
        3,
        amber,
    );
    registry.add(MWENGE.0, MWENGE.1, preset_two_phase(130.0));

    /* Nyerere Road: Tazara → Chang'Ombe → Gerezani (BRT Phase 2;
    starts outside the Tazara Nyerere dual E end at tx+430) */
    corridor_dual(
        curves,
        "Nyerere South corridor",
        &[
            (3691.0, 8647.0, Some(thru), Some(app)),
            (3820.0, 7400.0, None, None),
            (CHANGOMBE.0, CHANGOMBE.1, Some(app), Some(app)),
            (GEREZANI.0 - 60.0, GEREZANI.1 + 40.0, Some(app), Some(thru)),
        ],
        9.0,
        3,
        amber,
    );
    brt_median(
        curves,
        "BRT Ph-2 Nyerere South median",
        3700.0,
        8500.0,
        GEREZANI.0,
        GEREZANI.1,
        None,
    );
    registry.add(CHANGOMBE.0, CHANGOMBE.1, preset_two_phase(100.0));

    /* Gerezani area (§5.5.4): Bandari Road 34 m six-lane section
    (4 mixed + 2 BRT, Table 5.4.2) with the BRT-only flyover over
    Nyerere (Fig 5.4.5: 260 m bridge, 11 m width, 8 spans) and the
    Gerezani Bridge (Fig 5.4.10, 4-lane) toward the port. */
    corridor_dual(
        curves,
        "Bandari Road",
        &[
            (GEREZANI.0 + 40.0, GEREZANI.1 - 240.0, Some(thru), Some(app)),
            (GEREZANI.0, GEREZANI.1, Some(app), Some(app)),
            (4750.0, 5980.0, Some(thru), Some(thru)),
            (BANDARI_PORT.0, BANDARI_PORT.1, Some(app), Some(app)),
            (BANDARI_PORT.0 + 700.0, BANDARI_PORT.1 - 500.0, Some(app), Some(thru)),
        ],
        9.0,
        3,
        amber,
    );
    // BRT pair on the Bandari median (cement busway)
    brt_median(
        curves,
        "BRT Bandari median",
        GEREZANI.0,
        GEREZANI.1 - 220.0,
        BANDARI_PORT.0 + 650.0,
        BANDARI_PORT.1 - 460.0,
        None,
    );
    // BRT-only flyover of Nyerere Road (Fig 5.4.5): 260 m bridge on
    // 130 m transitions, 11 m wide, two BRT lanes, crown 6 m.
    flyover_jica(
        curves,
        "BRT flyover of Nyerere",
        GEREZANI.0 - 150.0,
        GEREZANI.1 - 180.0,
        GEREZANI.0 + 340.0,
        GEREZANI.1 + 160.0,
        9.0,
        2,
        6.0,
        [0x2a, 0x9a, 0x8a],
    );
    // Gerezani Bridge: the Bandari corridor crosses the rail/creek on
    // a 4-lane bridge (Fig 5.4.10) — lift that vertex to layer 1.
    let gerezani_bridge = (4750.0f64, 5980.0f64);
    for c in curves.iter_mut() {
        if c.name.starts_with("Bandari Road") {
            for p in &mut c.points {
                if (p.x - gerezani_bridge.0).hypot(p.z - gerezani_bridge.1) < 60.0 {
                    p.layer = 1;
                    p.bridge = true;
                    p.elev = 5.5;
                }
            }
        }
    }
    registry.add(GEREZANI.0, GEREZANI.1, preset_two_phase(120.0));
    registry.add(BANDARI_PORT.0, BANDARI_PORT.1, preset_two_phase(110.0));

    /* Nelson Mandela beltway east: Tazara → Ring East → Selanda Br
    (the report: "reinforcement of the Nelson Mandela Road beltway";
    EC-funded 16 km surface + drainage upgrade) */
    corridor_dual(
        curves,
        "Mandela East corridor",
        &[
            (TAZARA.0, TAZARA.1 + 430.0, Some(thru), Some(app)),
            (MANDELA_E.0, MANDELA_E.1, Some(app), Some(app)),
            (SELANDA.0, SELANDA.1, Some(app), Some(app)),
            (SELANDA.0 + 1400.0, SELANDA.1 - 300.0, Some(app), Some(thru)),
        ],
        9.0,
        3,
        amber,
    );
    registry.add(MANDELA_E.0, MANDELA_E.1, preset_two_phase(100.0));
    registry.add(SELANDA.0, SELANDA.1, preset_two_phase(110.0));

    /* Kilwa Road from Selanda south (4 lanes + median BRT reserve,
    Japan-funded upgrade §5.5.4) */
    corridor_dual(
        curves,
        "Kilwa Road",
        &[
            (SELANDA.0 + 80.0, SELANDA.1 - 40.0, Some(thru), Some(app)),
            (SELANDA.0 + 400.0, SELANDA.1 + 900.0, Some(thru), Some(thru)),
            (SELANDA.0 + 500.0, SELANDA.1 + 2400.0, Some(app), Some(thru)),
        ],
        8.0,
        2,
        amber,
    );
}

/* ---- Sam Nujoma × Igesa overlay ---- */

fn igesa_overlay(curves: &mut Vec<RoadCurve>, registry: &mut SignalRegistry) {
    let (ix, iz) = IGESA_XZ;
    // Igesa Road E-W into the Sam Nujoma dual — at-grade channelized
    dual(
        curves,
        "Igesa Road",
        ix - 240.0,
        iz,
        ix + 60.0,
        iz,
        5.5,
        2,
        Some(("through|through", "left|through|right")),
        None,
        [0x8a, 0x92, 0x62],
    );
    // right/left slip bays (channelization)
    slip(curves, "Igesa bay EB", ix, iz, -2.6, -1.9, 24.0);
    slip(curves, "Igesa bay WB", ix, iz, 0.6, 1.3, 24.0);
    registry.add(ix, iz, preset_two_phase(90.0));
}

/* ------------- turn-lane helpers (public API) ------------- */

/// Parse a JICA/OSM turn string into lane designations.
pub fn turns(s: &str) -> Vec<TurnDir> {
    parse_turn_lanes(s)
}

/// The JICA Table 5.2.1 / 5.3.1 standard approach: 1 left + 2 through
/// + 1 right, as an OSM turn:lanes string.
pub const APPROACH_1L2TH1R: &str = "left|through|through|right";

/// Named plan lookup for the Signals panel preset row.
pub fn preset_by_name(name: &str) -> Option<SignalPlan> {
    match name {
        "Ubungo BRT Ph-I 2-phase" => Some(preset_ubungo_2phase()),
        "Ubungo 4-phase optimized" => Some(crate::signals::preset_ubungo_4phase()),
        "Tazara 4-phase G20/Y3/AR2" => Some(preset_tazara_4phase()),
        "2-phase 90s" => Some(preset_two_phase(90.0)),
        "2-phase 130s" => Some(preset_two_phase(130.0)),
        _ => None,
    }
}

/* ------------- local (per-site) variants ------------- */

/// Per-site scenario: only the curves within `reach_m` of the site,
/// shifted so the site sits at the origin. Used for close-up renders /
/// focused editing (fog + grass-fan sizing stay sane).
pub fn build_dsm_local(site: (f64, f64), reach_m: f64) -> DsmScene {
    let full = build_dsm();
    let (sx, sz) = site;
    let cut = reach_m + 120.0;
    let mut curves: Vec<RoadCurve> = Vec::new();
    for c in full.curves {
        // clip long corridors at the reach circle: keep inside points and
        // interpolate entry/exit crossings so km-long arterial corridors
        // contribute clean fragments around the site (the corridors are
        // subdivided into <=420 m chunks by corridor_dual, so fragments
        // always carry several points)
        let (name_c, color_c, thick_c) = (c.name.clone(), c.color, c.underside_thickness);
        let dist = |p: &roads::model::RoadPoint| (p.x - sx).hypot(p.z - sz);
        let mut frag: Vec<roads::model::RoadPoint> = Vec::new();
        {
            let mut flush = |frag: &mut Vec<roads::model::RoadPoint>, curves: &mut Vec<RoadCurve>| {
                if frag.len() >= 2 {
                    let mut nc = RoadCurve::new(name_c.clone());
                    nc.color = color_c;
                    nc.underside_thickness = thick_c;
                    for p in frag.drain(..) {
                        nc.add_point(p);
                    }
                    curves.push(nc);
                } else {
                    frag.clear();
                }
            };
            for (i, p) in c.points.iter().enumerate() {
                if dist(p) <= cut {
                    if i > 0 {
                        if let Some(prev) = c.points.get(i - 1) {
                            if dist(prev) > cut {
                                let (d0, d1) = (dist(prev), dist(p));
                                let t = ((d0 - cut) / (d0 - d1).max(1e-6)).clamp(0.0, 1.0);
                                let mut q = RoadPoint::new(
                                    prev.x + (p.x - prev.x) * t - sx,
                                    prev.z + (p.z - prev.z) * t - sz,
                                );
                                q.auto_bearing = true;
                                q.lanes = p.lanes.max(1);
                                q.rev_lanes = p.rev_lanes;
                                q.oneway = p.oneway;
                                q.lane_width = p.lane_width;
                                q.terminated = true;
                                frag.push(q);
                            }
                        }
                    }
                    let mut q = p.clone();
                    q.x -= sx;
                    q.z -= sz;
                    frag.push(q);
                } else if let Some(last) = frag.last().map(|f| (f.x + sx, f.z + sz)) {
                    let d0 = (last.0 - sx).hypot(last.1 - sz);
                    let d1 = dist(p);
                    let t = ((cut - d0) / (d1 - d0).max(1e-6)).clamp(0.0, 1.0);
                    let mut q = RoadPoint::new(
                        last.0 + (p.x - last.0) * t - sx,
                        last.1 + (p.z - last.1) * t - sz,
                    );
                    q.auto_bearing = true;
                    q.lanes = p.lanes.max(1);
                    q.rev_lanes = p.rev_lanes;
                    q.oneway = p.oneway;
                    q.lane_width = p.lane_width;
                    q.terminated = true;
                    frag.push(q);
                    flush(&mut frag, &mut curves);
                } else {
                    flush(&mut frag, &mut curves);
                }
            }
            flush(&mut frag, &mut curves);
        }
    }
    let mut registry = full.registry;
    registry.entries.retain(|e| (e.x - sx).hypot(e.z - sz) <= reach_m);
    for e in &mut registry.entries {
        e.x -= sx;
        e.z -= sz;
    }
    let pois = full
        .pois
        .iter()
        .filter(|(_, x, z)| (x - sx).hypot(z - sz) <= reach_m)
        .map(|(n, x, z)| (n.clone(), x - sx, z - sz))
        .collect();
    DsmScene { curves, registry, pois }
}

/* ------------- R11 roadside enrichment (RoadArchitect style) ------------- */

use crate::buildings::{blocks_for_sites, Site};
use crate::props::{join_along, stamp, JoinParams, PropInstance};

/// Flag curves for RoadArchitect-style W-beam guardrails (docs §2):
/// flyover approach ramps, slip roads and BRT corridors. Logic-restricted —
/// arterials at grade stay open.
///
/// R12 also flags the RA `GSDCableBarrier` on the median curves: any
/// guardrail-flagged curve that carries a real median (max point median
/// ≥ 2.0 m — station flares / wide medians) or is a BRT median busway
/// of the Sam Nujoma / Morogoro / Nelson Mandela corridors gets the
/// flexible cable barrier between the carriageways.
pub fn apply_guardrails(curves: &mut [RoadCurve]) {
    for c in curves.iter_mut() {
        let n = c.name.to_lowercase();
        c.guardrail = n.contains("flyover")
            || n.contains("slip")
            || n.contains("brt")
            || n.contains("ramp")
            || n.contains("bridge");
        let med = c.points.iter().map(|p| p.median).fold(0.0f64, f64::max);
        c.cable_barrier = c.guardrail && (med >= 2.0 || (n.contains("brt") && med > 0.0));
    }
}

/// R12 RA bridge kit styles: the elevated Selanda bridge curve (Mandela
/// × Kilwa) carries the suspension kit. Only applied when an ELEVATED
/// curve named "Selanda" actually exists — the current OSM/JICA network
/// builds Selanda as an at-grade channelized junction, so this is a
/// documented no-op until a grade-separated Selanda crossing lands
/// (synthesizing a demo curve here is not allowed).
pub fn apply_bridge_styles(curves: &mut [RoadCurve]) {
    for c in curves.iter_mut() {
        if c.name.to_lowercase().contains("selanda") && c.has_elevated() {
            c.bridge_style = BridgeStyle::Suspension;
        }
    }
}

/// BACKUP — roadside prop stamping is PARKED (R14: the app shows ONLY
/// roads + bridges; no trees, no palms, no streetlights, no signs, no
/// workzone cones/barrels/attenuators, no BRT station props). The whole
/// generator stays here — flip this flag to bring the dressing back.
pub const PROPS_ENABLED: bool = false;

/// Roadside props (the "edge objects" of RA — docs §2): streetlights every
/// ~36 m on arterials, acacia/palm tree runs, speed-limit signs, and the
/// BRT station props at the modelled station sites.
pub fn dsm_props_capped(curves: &[RoadCurve], max_range: f64) -> Vec<PropInstance> {
    if !PROPS_ENABLED {
        return Vec::new();
    }
    let mut out: Vec<PropInstance> = Vec::new();
    const TREE_CAP: usize = 950;
    let mut tree_count = 0usize;
    let mut sign_i = 0u64;
    for c in curves {
        let n = c.name.to_lowercase();
        let walk = n.contains("walkway") || n.contains("cycleway") || n.contains("path");
        let elevated = c.has_elevated();
        let paved = c.paved_width();
        let arterial = paved >= 9.0 && !walk;

        // streetlights (RA GSDStreetLightSingle: MeterSep 30 m, edge offset)
        if arterial {
            out.extend(join_along(
                c,
                "streetlight",
                &JoinParams {
                    spacing: 36.0,
                    lateral: paved / 2.0 + 1.0,
                    alternate: true,
                    align: true,
                    scale: 1.0,
                    jitter: 0.05,
                    random_yaw: false,
                    seed: 0x51AF,
                },
            ));
            // speed-limit signs every ~280 m (RA FedSign singles)
            let len = c.length_m();
            let mut s = 90.0;
            while s < len - 40.0 {
                if let Some(p) = stamp(c, if sign_i % 2 == 0 { "signSpeed50" } else { "signSpeed80" }, s / len, paved / 2.0 + 2.2, true, 1.0) {
                    out.push(p);
                }
                sign_i += 1;
                s += 280.0;
            }
        }

        // trees — ground roads only (decks carry parapets, not gardens)
        if !elevated && !walk && tree_count < TREE_CAP {
            for (asset, spacing, seed) in [
                ("tree", 21.0, 0xACAC),
                ("palm", 44.0, 0x1A1E),
            ] {
                let props = join_along(
                    c,
                    asset,
                    &JoinParams {
                        spacing,
                        lateral: paved / 2.0 + 3.8,
                        alternate: true,
                        align: false,
                        scale: 1.15,
                        jitter: 0.35,
                        random_yaw: true,
                        seed,
                    },
                );
                tree_count += props.len();
                out.extend(props);
                if tree_count >= TREE_CAP {
                    break;
                }
            }
        }
    }

    // BRT station props at the modelled stations (JICA Ph-1 flare 150 m W)
    for c in curves {
        if c.name.starts_with("BRT Ph-1 Morogoro median") {
            if let Some(p) = stamp(c, "brtStation", 0.3256, 0.0, true, 1.0) {
                out.push(p);
            }
        }
        if c.name.starts_with("BRT Ph-2 Nyerere median") {
            if let Some(p) = stamp(c, "brtShelterSmall", 0.5, 0.0, true, 1.0) {
                out.push(p);
            }
        }
    }

    // R12 Igesa BRT Phase-4 rebuild workzone (RA Interactive props:
    // TrafficCone / Barrels / GSDAtten) — exported separately as
    // `dsm_workzone` so every caller of dsm_props_capped gets it
    out.extend(dsm_workzone(curves));

    // junction-clearing gate: drop props standing on crossing pavement
    // (any two curves' points within 12 m); local close-ups also cap the
    // range so nothing renders at the fog edge (floating-trees artifact)
    out.retain(|p| {
        if (p.pos.x as f64).hypot(p.pos.z as f64) > max_range {
            return false;
        }
        p.asset != "tree" || !near_crossing(curves, p.pos.x as f64, p.pos.z as f64, 12.0)
    });
    out
}

/// Full-scene props (no range cap — the app recenter keeps everything).
pub fn dsm_props(curves: &[RoadCurve]) -> Vec<PropInstance> {
    dsm_props_capped(curves, f64::INFINITY)
}

/// R12 Igesa BRT Phase-4 rebuild workzone (RA Interactive props:
/// TrafficCone / Barrels / GSDAtten): cones every 6 m (tangent yaw,
/// alternating ±paved/2+1.5) + drums every 18 m (lateral +paved/2+2.5)
/// along the first/last 120 m of every Igesa curve, plus one impact
/// attenuator per end of the first Igesa curve (+3 m lateral). Totals
/// are capped at ~140 cones / 50 barrels.
pub fn dsm_workzone(curves: &[RoadCurve]) -> Vec<PropInstance> {
    let mut out = Vec::new();
    const ZONE_M: f64 = 120.0;
    const CONE_CAP: usize = 140;
    const BARREL_CAP: usize = 50;
    let mut cones = 0usize;
    let mut barrels = 0usize;
    let mut first_done = false;
    for c in curves {
        if !c.name.to_lowercase().contains("igesa") {
            continue;
        }
        let len = c.length_m();
        if len < 8.0 {
            continue;
        }
        let paved = c.paved_width();
        let in_zone = |s: f64| s < ZONE_M || s > len - ZONE_M;
        // cones every 6 m, alternating ±(paved/2 + 1.5), tangent-aligned
        let mut side = 1.0f64;
        let mut s = 3.0;
        while s < len && cones < CONE_CAP {
            if in_zone(s) {
                if let Some(p) = stamp(c, "cone", s / len, (paved / 2.0 + 1.5) * side, true, 1.0) {
                    out.push(p);
                    cones += 1;
                }
                side = -side;
            }
            s += 6.0;
        }
        // channelizer drums every 18 m on the +lateral side
        let mut s = 9.0;
        while s < len && barrels < BARREL_CAP {
            if in_zone(s) {
                if let Some(p) = stamp(c, "barrel", s / len, paved / 2.0 + 2.5, true, 1.0) {
                    out.push(p);
                    barrels += 1;
                }
            }
            s += 18.0;
        }
        // impact attenuators at the workzone entry: one per end of the
        // FIRST Igesa curve, +3 m off the centerline
        if !first_done {
            for t in [3.0 / len, (len - 3.0) / len] {
                if let Some(p) = stamp(c, "attenuator", t, 3.0, true, 1.0) {
                    out.push(p);
                }
            }
            first_done = true;
        }
    }
    out
}

/// True when (x, z) sits within `r` of ANOTHER curve's samples (crossing
/// pavement) — used to keep trees off junction pads.
fn near_crossing(curves: &[RoadCurve], x: f64, z: f64, r: f64) -> bool {
    let mut hits = 0;
    for c in curves {
        for p in &c.points {
            if (p.x - x).hypot(p.z - z) < r {
                hits += 1;
                if hits >= 2 {
                    return true;
                }
            }
        }
    }
    false
}

/// JICA site blocks for the procedural buildings (docs §7):
/// Kijazi/Ubungo commercial, Sinza residential, Tazara, Mwenge market,
/// Gerezani/Bandari, Selanda.
pub fn dsm_building_sites() -> Vec<Site> {
    vec![
        (0.0, 0.0, 420.0, true),               // Kijazi / Ubungo
        (IGESA_XZ.0, IGESA_XZ.1, 340.0, false), // Sinza residential
        (TAZARA.0, TAZARA.1, 340.0, false),     // Tazara
        (MWENGE.0, MWENGE.1, 280.0, true),      // Mwenge market
        (GEREZANI.0, GEREZANI.1, 260.0, false), // Gerezani / Bandari
        (SELANDA.0, SELANDA.1, 220.0, false),   // Selanda
    ]
}

/// BACKUP — procedural city blocks are PARKED (R13: the app UI is roads +
/// bridges only; buildings are not implemented for now, the generator stays
/// here so the feature can be re-enabled by flipping this flag).
pub const BUILDINGS_ENABLED: bool = false;

/// Procedural city blocks (world-space mesh parts) for the JICA sites.
pub fn dsm_buildings(curves: &[RoadCurve]) -> Vec<roads::mesh3d::MeshPart> {
    if !BUILDINGS_ENABLED {
        return Vec::new();
    }
    blocks_for_sites(curves, &dsm_building_sites(), 0xD5_1A57)
}
