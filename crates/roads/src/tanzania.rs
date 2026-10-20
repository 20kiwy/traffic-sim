//! Tanzania / JICA-TR2 design parameters — every width, span and grade
//! below is sourced from the JICA "Dar es Salaam Transport Policy and
//! System Development Master Plan" Technical Report 2 (11897576_02.pdf)
//! and standard TANROADS urban practice, so DSM scenes build at
//! engineering scale rather than guesswork.
//!
//! Sources (page refs = PDF pages of 11897576_02.pdf):
//! - Table 5.4.2 (p25): 34 m six-lane BRT cross-section, 27 m four-lane,
//!   PC hollow girder bridge 260 m / 11 m / 8 spans, T-type PC girder
//!   35 m / 34 m, box culvert 3.5 × 5.0 m.
//! - Fig 5.3.10 (p12): Tazara flyover concept design — bridge 300 m
//!   (190 m PC box girder + 2 × 60 m PC hollow slabs), approaches
//!   137.303 m / 175.408 m, vertical curves + gradients.
//! - Fig 5.3.11 (p13/30): Tazara plan S=1:1000 — 35/50/35 m zoning,
//!   bus lanes + zebra bars on Nyerere approaches, right-turn lanes
//!   under the flyover.
//! - Fig 5.4.5 (p20): BRT-only flyover at Nyerere — 260 m bridge,
//!   8 spans, 130 m transitions, 11 m BRT width (Table 5.4.2).
//! - Fig 5.5.1 (p26): priority intersection map + road-class legend.
//! - §5.5.4 / Fig 5.4.11 (p23): Gerezani bridge (4-lane), BRT Harbour
//!   Bridge (2-lane) with typical cross section.

/// Standard through-lane widths (m). JICA urban carriageways: 9.5 m for
/// 3 lanes ⇒ 3.17 m; arterials default 3.2 m, interchange ramps 3.5 m,
/// local streets 3.0 m.
pub const LANE_ARTERIAL: f64 = 3.2;
pub const LANE_RAMP: f64 = 3.5;
pub const LANE_LOCAL: f64 = 3.0;
/// JICA Table 5.4.2 six-lane section carriageway: 9.5 m / 3 lanes.
pub const LANE_JICA_SIXLANE: f64 = 9.5 / 3.0;

/// Sidewalk / footpath width on arterials (JICA: sidewalk @ 3 m;
/// 27 m section footpaths @ 3 m).
pub const WALK_ARTERIAL: f64 = 3.0;
/// Paved shoulder on dual carriageways (m).
pub const SHOULDER_ARTERIAL: f64 = 0.5;
/// Six-lane BRT section median (JICA Table 5.4.2: 9 m for 2-way BRT).
pub const MEDIAN_BRT_SIXLANE: f64 = 9.0;
/// Four-lane section raised median (JICA 27 m section).
pub const MEDIAN_FOURLANE: f64 = 2.0;
/// BRT busway lane width (JICA BRT bridge: 9 m for two BRT lanes).
pub const LANE_BRT: f64 = 4.5;
/// BRT bridge walkway for pedestrians + NMV (m).
pub const BRT_BRIDGE_WALK: f64 = 2.5;
/// BRT bridge total deck width (9 m busway + 2.5 m walkway).
pub const BRT_BRIDGE_WIDTH: f64 = 9.0 + BRT_BRIDGE_WALK;

/// Zebra (pedestrian) crossing geometry — JICA plans show zebra bars on
/// every signalized approach (Fig 5.3.11 "PLAN S=1:1,000").
pub const ZEBRA_WIDTH: f64 = 3.0; // across the road (m)
pub const ZEBRA_STRIPE: f64 = 0.5; // bar width along the road (m)
pub const ZEBRA_GAP: f64 = 0.5; // gap between bars (m)
/// Stop-line setback from the junction edge (m).
pub const STOP_LINE_SETBACK: f64 = 2.0;

/// Speed limits (km/h) — urban arterial 50 (TANROADS), interchange
/// ramps 40, BRT corridor 50 with 30 at stations.
pub const SPEED_ARTERIAL_KMH: f64 = 50.0;
pub const SPEED_RAMP_KMH: f64 = 40.0;
pub const SPEED_BRT_KMH: f64 = 50.0;

/// Roadside open drain (TANROADS roadside practice, JICA Nelson
/// Mandela "enhancements of road surface, drainage" §5.5.3):
/// trapezoidal channel, bottom width × depth, side slope 1:1.
pub const DRAIN_BOTTOM: f64 = 0.6;
pub const DRAIN_DEPTH: f64 = 0.7;
/// Distance from paved edge to drain centreline (m).
pub const DRAIN_OFFSET: f64 = 2.2;
/// Kerb + gutter on urban raised sections (m width).
pub const KERB_GUTTER: f64 = 0.45;

/// JICA box culvert (Table 5.4.2): 3.5 m × 5.0 m single shell.
pub const CULVERT_W: f64 = 3.5;
pub const CULVERT_H: f64 = 5.0;

/// Vertical clearances: bridge deck above the under-road (min 5.5 m
/// urban; JICA flyovers ride ~7 m above grade at the crown).
pub const CLEARANCE_URBAN: f64 = 5.5;
pub const FLYOVER_CROWN: f64 = 7.0;

/* ---------- Tazara flyover concept design (Fig 5.3.10) ---------- */

/// BRIDGE LENGTH L=300.000 m.
pub const TAZARA_BRIDGE_LEN: f64 = 300.0;
/// Approach lengths L=137.303 (west) / L=175.408 (east).
pub const TAZARA_APPROACH_W: f64 = 137.303;
pub const TAZARA_APPROACH_E: f64 = 175.408;
/// Span assembly (bridge 300 m total): PC hollow slab approach spans
/// (2 × 45 m each side) + the 120 m PC box girder main span over
/// Nyerere Road (Fig 5.3.11 plan: "PC Box Girder L=120.000").
pub const TAZARA_SPANS: [(&str, f64); 5] = [
    ("PC hollow slab", 45.0),
    ("PC hollow slab", 45.0),
    ("PC box girder", 120.0),
    ("PC hollow slab", 45.0),
    ("PC hollow slab", 45.0),
];
/// Flyover approach grades (Fig 5.3.10 gradient row): 1.1 % up, 3.5 %
/// to the crest, ~5 % over the main span, 4.0 % down, 1.1 % out.
pub const TAZARA_GRADES: [f64; 5] = [0.011, 0.035, 0.05, 0.04, 0.011];
/// Flyover carriageway: two 2-lane decks, 17 m total width (§5.3.5).
pub const TAZARA_DECK_LANES: usize = 2;
pub const TAZARA_DECK_HALF_SEPARATION: f64 = 9.0;

/* ---------- BRT-only flyover at Nyerere (Fig 5.4.5 + Table 5.4.2) ---------- */

/// PC hollow girder 260 m + 265 m transitions, 11 m BRT width, 8 spans.
pub const BRT_FLYOVER_BRIDGE_LEN: f64 = 260.0;
pub const BRT_FLYOVER_TRANSITION: f64 = 265.0;
pub const BRT_FLYOVER_WIDTH: f64 = 11.0;
pub const BRT_FLYOVER_SPANS: f64 = 8.0;

/* ---------- Gerezani / Bandari (§5.5.4 + Fig 5.4.10/5.4.11) ---------- */

/// Bandari Road widening: 1.67 km to six lanes (4 mixed + 2 BRT).
pub const BANDARI_WIDEN_LEN: f64 = 1670.0;
/// Sokoine Drive widening (Option B): 1.45 km to four lanes.
pub const SOKOINE_WIDEN_LEN: f64 = 1450.0;
/// Bandari/Gerezani improvement length (Table 5.4.2: 1300 m + 370 m).
pub const GEREZANI_ROAD_LEN: f64 = 1300.0;
/// Gerezani Bridge: 4 lanes (Fig 5.4.10).
pub const GEREZANI_BRIDGE_LANES: usize = 4;
/// BRT Harbour / Port Bridge: 2 BRT lanes (Fig 5.4.11).
pub const PORT_BRIDGE_LANES: usize = 2;
/// T-type PC girder single span (Table 5.4.2): 35 m long, 34 m wide.
pub const T_GIRDER_SPAN: f64 = 35.0;
pub const T_GIRDER_WIDTH: f64 = 34.0;

/* ---------- priority intersections (Fig 5.5.1) ---------- */

/// The Fig 5.5.1 targeted set, in JICA priority order, with the kind of
/// solution the report recommends. Used by the DSM builder so every
/// listed junction exists with its turn lanes.
pub const PRIORITY_INTERSECTIONS: [(&str, &str, bool); 9] = [
    // (name, roads, flyover recommended?)
    ("Ubungo (Kijazi)", "Morogoro × Sam Nujoma × Nelson Mandela", true),
    ("Tazara", "Nyerere × Nelson Mandela", true),
    ("Gerezani (Bandari)", "Kilwa × Bandari × Sokoine", false),
    ("Mwenge", "Sam Nujoma × New Bagamoyo", true),
    ("Morocco", "Morogoro × Morocco", false),
    ("Magomeni", "Morogoro × Magomeni", false),
    ("Chang'Ombe", "Nyerere × Chang'ombe", false),
    ("Selanda Bridge", "Nelson Mandela × Kilwa", false),
    ("Mandela Ring East", "Nelson Mandela × Nelson Mandela link", false),
];

/// Approach string for every JICA-reviewed arterial approach
/// (Table 5.2.1 / 5.3.1: 1 left + 2 through + 1 right).
pub const APPROACH_1L2TH1R: &str = "left|through|through|right";

/// The 34 m six-lane BRT cross-section (Table 5.4.2, Option A):
/// 3 m walk | 9.5 m carriageway | 9 m BRT median | 9.5 m carriageway |
/// 3 m walk.
pub struct SixLaneBrt;
impl SixLaneBrt {
    pub const TOTAL_W: f64 = 34.0;
    pub const CARRIAGEWAY: f64 = 9.5;
    pub const MEDIAN: f64 = 9.0;
    pub const WALK: f64 = 3.0;
    pub const LANES_PER_DIR: usize = 3;
}

/// The 27 m four-lane cross-section (Table 5.4.2, Option C):
/// 3 m footpath | 9.5 m | 2 m median | 9.5 m | 3 m footpath.
pub struct FourLaneMedian;
impl FourLaneMedian {
    pub const TOTAL_W: f64 = 27.0;
    pub const CARRIAGEWAY: f64 = 9.5;
    pub const MEDIAN: f64 = 2.0;
    pub const WALK: f64 = 3.0;
    pub const LANES_PER_DIR: usize = 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jica_sections_add_up() {
        // 34 m: walk + 9.5 + 9 + 9.5 + walk
        let six = SixLaneBrt::WALK + SixLaneBrt::CARRIAGEWAY + SixLaneBrt::MEDIAN
            + SixLaneBrt::CARRIAGEWAY + SixLaneBrt::WALK;
        assert!((six - SixLaneBrt::TOTAL_W).abs() < 0.01, "six-lane section sums to 34 m");
        let four = FourLaneMedian::WALK + FourLaneMedian::CARRIAGEWAY
            + FourLaneMedian::MEDIAN + FourLaneMedian::CARRIAGEWAY + FourLaneMedian::WALK;
        assert!((four - FourLaneMedian::TOTAL_W).abs() < 0.01, "27 m section sums");
    }

    #[test]
    fn tazara_spans_fill_the_bridge() {
        let total: f64 = TAZARA_SPANS.iter().map(|(_, l)| l).sum();
        assert!((total - TAZARA_BRIDGE_LEN).abs() < 0.01, "spans sum to 300 m");
    }

    #[test]
    fn priority_list_complete() {
        assert_eq!(PRIORITY_INTERSECTIONS.len(), 9);
        assert!(PRIORITY_INTERSECTIONS.iter().any(|(n, _, f)| *n == "Tazara" && *f));
    }
}
