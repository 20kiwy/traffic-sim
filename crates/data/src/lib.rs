//! tsdata — raw road-data providers for the traffic-sim workspace.
//!
//! Everything the sim needs to go from *the real world* to GRG road
//! curves and terrain:
//!
//! * [`osm`] — OpenStreetMap 0.6 API + Overpass providers (curl-backed
//!   [`osm::http_get`] with file caching, mirror rotation, and an
//!   in-memory [`osm::OsmIndex`] over OSM JSON documents).
//! * [`mapbox`] — Mapbox vector/terrain/satellite tile URLs + fetchers
//!   and a hand-rolled MVT decoder (protobuf wire format parsed
//!   manually — no protobuf dependency).
//! * [`dem`] — Terrarium + Mapbox Terrain-RGB elevation tiles with
//!   bilinear sampling in local tiles.
//! * [`import`] — the bridge to the `roads` crate: OSM documents →
//!   `RoadCurve`/`RoadPoint` in local meters (x east, z south), bridge
//!   feature extraction, and OFFLINE demo imports for the Kijazi and
//!   Igesa × Sam Nujoma areas.
//! * [`probe`] — human-readable diagnostics for the integrator CLI.
//! * [`token`] — the embedded Mapbox public token (with env override).
//!
//! Network note: std has no TLS, so fetching shells out to the system
//! `curl` binary (cached, UA `traffic-sim/0.4 (road data import)`); all
//! demo entry points also work fully offline from `assets/data`.

pub mod dem;
pub mod import;
pub mod mapbox;
pub mod osm;
pub mod probe;
pub mod token;
pub mod types;

// Convenience re-exports for the common entry points.
pub use dem::DemGrid;
pub use osm::http_get;
pub use types::{BridgeFeature, GeoBounds, ImportStats, SurfaceKind};
