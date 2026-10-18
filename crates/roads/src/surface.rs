//! Road surface types — the pavement material of a RoadPoint.
//!
//! GRG models only two visual states (the trim-sheet asphalt vs. the
//! `dirt` toggle); this module extends the same idea into an explicit
//! per-point surface id that scripts/UI can set and query. The id maps
//! onto the roadkit texture slots and drives:
//! - junction annulus / patch textures (via `RoadCurve::dominant_surface`,
//!   e.g. a CEMENT BRT corridor gives its roundabout ring a cement ring),
//! - gutter generation (unpaved surfaces — GRAVEL / MUD — get no
//!   concrete gutters, GRG-style),
//! - the `roads::surface::{ASPHALT, ...}` constants other crates
//!   (crates/data, roadkit UI) key their road materials on.

/// Paved asphalt (default; TEX_ASPHALT).
pub const ASPHALT: u8 = 0;
/// Cast concrete / BRT corridor slab (TEX_CEMENT).
pub const CEMENT: u8 = 1;
/// Concrete pavement (TEX_CEMENT).
pub const CONCRETE: u8 = 2;
/// Historic stone setts (TEX_ASPHALT with a darker tint as fallback).
pub const COBBLE: u8 = 3;
/// Unpaved compacted gravel (TEX_ROUGH) — no gutters.
pub const GRAVEL: u8 = 4;
/// Unpaved mud / earth road (TEX_MUD) — no gutters.
pub const MUD: u8 = 5;

/// Display names, indexed by surface id.
pub const NAMES: [&str; 6] = ["asphalt", "cement", "concrete", "cobble", "gravel", "mud"];

/// Display name of a surface id (out-of-range ids fall back to asphalt).
pub fn name(s: u8) -> &'static str {
    NAMES.get(s as usize).copied().unwrap_or(NAMES[0])
}

/// Parse a surface name (case-insensitive, whitespace trimmed); unknown
/// names resolve to ASPHALT. For scripts / UI pickers.
pub fn from_name(s: &str) -> u8 {
    let low = s.trim().to_ascii_lowercase();
    for (i, n) in NAMES.iter().enumerate() {
        if *n == low {
            return i as u8;
        }
    }
    ASPHALT
}

/// Unpaved surfaces: no concrete gutters (GRG-style dirt roads carry
/// none), rough junction annulus textures, no painted markings.
pub fn is_unpaved(s: u8) -> bool {
    matches!(s, GRAVEL | MUD)
}

/// Texture slot (roadkit::textures order) + tint for a surface id, used
/// by the junction patches: the tint multiplies over the texture the same
/// way the road strips' tints do. COBBLE falls back to the asphalt slot
/// with a darker warm (stone) tint.
pub fn tex_and_tint(s: u8) -> (i32, [f32; 4]) {
    match s {
        CEMENT | CONCRETE => (crate::mesh3d::TEX_CEMENT, crate::mesh3d::CONCRETE_TINT),
        COBBLE => (crate::mesh3d::TEX_ASPHALT, [1.15, 1.08, 1.0, 1.0]),
        GRAVEL => (crate::mesh3d::TEX_ROUGH, crate::mesh3d::ROUGH_TINT),
        MUD => (crate::mesh3d::TEX_MUD, crate::mesh3d::MUD_TINT),
        _ => (crate::mesh3d::TEX_ASPHALT, crate::mesh3d::ROAD_TINT),
    }
}
