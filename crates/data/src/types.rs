//! Shared data types: geographic bounds, surface classification, bridge
//! features and import statistics.

use serde_json::Value;

/// Geographic bounding box in decimal degrees (WGS84).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeoBounds {
    pub south: f64,
    pub west: f64,
    pub north: f64,
    pub east: f64,
}

impl GeoBounds {
    /// True when the (lat, lon) point lies inside (inclusive) the box.
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        lat >= self.south && lat <= self.north && lon >= self.west && lon <= self.east
    }

    /// (lat, lon) midpoint of the box.
    pub fn center(&self) -> (f64, f64) {
        ((self.south + self.north) / 2.0, (self.west + self.east) / 2.0)
    }

    /// Kijazi bridge area — Morogoro Road × Nelson Mandela Road flyover,
    /// Dar es Salaam (the John Kijazi Bridge / flyover OSM extracts).
    pub fn kijazi_area() -> Self {
        Self { south: -6.8000, west: 39.2040, north: -6.7890, east: 39.2125 }
    }

    /// Igesa Road × Sam Nujoma Road junction area (Sinza / Ubungo,
    /// Dar es Salaam).
    pub fn igesa_samnujoma_area() -> Self {
        Self { south: -6.7830, west: 39.2140, north: -6.7640, east: 39.2320 }
    }

    /// Greater Dar es Salaam.
    pub fn dar_es_salaam() -> Self {
        Self { south: -6.95, west: 39.00, north: -6.55, east: 39.45 }
    }
}

/// Road surface classification from OSM `surface=*` tags.
///
/// [`SurfaceKind::as_u8`] encodes into the same numeric constants the
/// `roads` crate exposes in `roads::surface` (ASPHALT=0, CEMENT=1,
/// CONCRETE=2, COBBLE=3, GRAVEL=4, MUD=5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceKind {
    Asphalt,
    Cement,
    Concrete,
    Cobble,
    Gravel,
    Mud,
    Compacted,
    Unknown,
}

impl SurfaceKind {
    /// Map an OSM `surface=*` tag value to a [`SurfaceKind`].
    ///
    /// Spec mapping (unknown values → [`SurfaceKind::Unknown`]):
    /// - `asphalt` → Asphalt
    /// - `cement`, `concrete:cement` → Cement; `concrete` → Concrete
    /// - `paving_stones`, `cobblestone`, `sett` → Cobble
    /// - `gravel`, `fine_gravel`, `pebblestone` → Gravel
    /// - `mud`, `dirt`, `ground`, `earth` → Mud
    /// - `compacted` → Compacted
    pub fn from_tag(t: &str) -> SurfaceKind {
        match t.trim().to_ascii_lowercase().as_str() {
            "asphalt" => SurfaceKind::Asphalt,
            "cement" | "concrete:cement" => SurfaceKind::Cement,
            "concrete" => SurfaceKind::Concrete,
            "paving_stones" | "cobblestone" | "sett" => SurfaceKind::Cobble,
            "gravel" | "fine_gravel" | "pebblestone" => SurfaceKind::Gravel,
            "mud" | "dirt" | "ground" | "earth" => SurfaceKind::Mud,
            "compacted" => SurfaceKind::Compacted,
            _ => SurfaceKind::Unknown,
        }
    }

    /// Numeric encoding matching the `roads::surface` constants
    /// (ASPHALT=0, CEMENT=1, CONCRETE=2, COBBLE=3, GRAVEL=4, MUD=5).
    ///
    /// * `Compacted` maps to GRAVEL — the roads crate documents that
    ///   constant as "unpaved compacted gravel" (TEX_ROUGH). The
    ///   [`SurfaceKind::is_dirt`] flag (Mud + Compacted) still drives
    ///   `RoadPoint.dirt` for the mud-lane / rough-shoulder rendering.
    /// * `Unknown` maps to ASPHALT — OSM's implicit default for major
    ///   urban roads without a surface tag, matching
    ///   `roads::surface::from_name`'s fallback for unknown names.
    pub fn as_u8(&self) -> u8 {
        use roads::surface::{ASPHALT, CEMENT, COBBLE, CONCRETE, GRAVEL, MUD};
        match self {
            SurfaceKind::Asphalt => ASPHALT,
            SurfaceKind::Cement => CEMENT,
            SurfaceKind::Concrete => CONCRETE,
            SurfaceKind::Cobble => COBBLE,
            SurfaceKind::Gravel => GRAVEL,
            SurfaceKind::Mud => MUD,
            SurfaceKind::Compacted => GRAVEL,
            SurfaceKind::Unknown => ASPHALT,
        }
    }

    /// True for the unpaved-earth family (drives `RoadPoint.dirt`).
    pub fn is_dirt(&self) -> bool {
        matches!(self, SurfaceKind::Mud | SurfaceKind::Compacted)
    }
}

/// A bridge / viaduct way extracted from an OSM document.
#[derive(Clone, Debug)]
pub struct BridgeFeature {
    /// OSM way id.
    pub way_id: i64,
    /// `name` tag (may be empty).
    pub name: String,
    /// Raw `bridge` tag value: `viaduct` | `bridge` | `yes` | ...
    pub bridge: String,
    /// `layer` tag as an integer (default 0).
    pub layer: i32,
    /// `highway` tag value.
    pub highway: String,
    /// True when the way carries BRT infrastructure (`lanes:psv` present
    /// or highway is `busway` / `bus_guideway`).
    pub is_brt: bool,
    /// Node coordinates as `[lat, lon]` pairs (same order as
    /// `OsmIndex::node_pos`).
    pub nodes: Vec<[f64; 2]>,
    /// Full OSM tag map.
    pub tags: serde_json::Map<String, Value>,
}

/// Aggregate counters produced by an import run.
#[derive(Clone, Debug, Default)]
pub struct ImportStats {
    /// Highway ways that passed the keep-filter.
    pub ways: usize,
    /// Road curves successfully built.
    pub curves: usize,
    /// Bridge features extracted.
    pub bridges: usize,
    /// Kept ways that are BRT (`lanes:psv` / busway / bus_guideway).
    pub brt_ways: usize,
    /// Min sampled elevation (m); 0.0 when no DEM was provided.
    pub elev_min: f32,
    /// Max sampled elevation (m); 0.0 when no DEM was provided.
    pub elev_max: f32,
}
