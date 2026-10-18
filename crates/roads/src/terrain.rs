//! RoadTerrain3DConnector — the GRG terrain bridge (`connectors/
//! terrain3d_road_connector.gd`) adapted to RoadKit's procedural
//! heightfield. In Godot the connector flattens Terrain3D maps under
//! roads; here the whole ground IS the heightfield, so the connector:
//!
//! 1. defines rolling-hill terrain (deterministic, seeded sine mix),
//! 2. flattens it under every road: within the paved half-width plus
//!    `edge_margin` the ground snaps to the road elevation (+`offset`),
//!    then eases back to the hills over `edge_falloff` meters
//!    (GRG APPROXIMATE method — fast, smooth falloff),
//! 3. reports the height at any point for agents / props.
//!
//! The properties mirror the Godot connector: `offset` (-0.25),
//! `edge_margin` (0.5), `edge_falloff` (2.0).

/// Connector settings + procedural hill parameters (Scene-level, like the
/// single RoadTerrain3DConnector node in a Godot scene).
#[derive(Clone, Debug)]
pub struct TerrainState {
    /// connector present + enabled (the "Set terrain" history action)
    pub enabled: bool,
    /// hill amplitude (meters) — 0 = flat plain
    pub amplitude: f64,
    /// hill wavelength scale (meters between crests)
    pub wavelength: f64,
    pub seed: u64,
    /// vertical offset to avoid z-fighting (negative sinks under the road)
    pub offset: f64,
    /// extra flattening beyond the road edge (meters)
    pub edge_margin: f64,
    /// falloff distance beyond edge+margin back to the hills (meters)
    pub edge_falloff: f64,
    /// flatten using point elevations (false = keep hills, only offset)
    pub flatten_under_roads: bool,
}

impl Default for TerrainState {
    fn default() -> Self {
        Self {
            enabled: false,
            amplitude: 6.0,
            wavelength: 130.0,
            seed: 20260207,
            offset: -0.25,
            edge_margin: 0.5,
            edge_falloff: 2.0,
            flatten_under_roads: true,
        }
    }
}

impl TerrainState {
    /// Raw hill height at (x, z) — deterministic 3-wave sine mix.
    pub fn hill_height(&self, x: f64, z: f64) -> f64 {
        if self.amplitude <= 0.0 {
            return 0.0;
        }
        let l = self.wavelength.max(10.0);
        let s = self.seed as f64 * 0.173;
        let a = (x / l + s).sin() * (z / l * 0.83 + s * 1.7).cos();
        let b = (x / (l * 0.41) + s * 2.3).cos() * (z / (l * 0.53) + s).sin();
        let c = ((x + z) / (l * 1.9) + s * 0.5).sin();
        self.amplitude * (a * 0.62 + b * 0.28 + c * 0.10)
    }

    /// Height WITHOUT road flattening (hills).
    pub fn height_at(&self, x: f64, z: f64) -> f64 {
        if !self.enabled {
            0.0
        } else {
            self.hill_height(x, z)
        }
    }
}
