//! Terrain baking — the CPU port of egregoria's `HeightmapRender` pipeline,
//! and the SquareStage: the single square ground patch that replaces
//! egregoria's streamed terrain chunks. There are no terrain chunks to fetch
//! or stream here; the square shows the roads.

use crate::pem::{pack_height, unpack_height, Pem, RESOLUTION};
use gdmath::Vec3;
use std::sync::OnceLock;

/// the mercator projection anchor (center of the imported map area)
static ANCHOR: OnceLock<(f64, f64)> = OnceLock::new();

/// Set the lat/lon anchor used by all local-meters <-> lat/lon conversions.
/// Must be called once before any data import (the CLI glue does it).
pub fn set_anchor(lat0: f64, lon0: f64) {
    let _ = ANCHOR.set((lat0, lon0));
}

pub fn anchor() -> (f64, f64) {
    *ANCHOR.get_or_init(|| (-6.79, 39.21)) // Dar es Salaam default
}

const R: f64 = 6371000.0;

/// local meters (x east, z south) -> (lat, lon)
pub fn local_to_latlon(x: f64, z: f64) -> (f64, f64) {
    let (lat0, lon0) = anchor();
    let lat = lat0 - (z / R).to_degrees();
    let lon = lon0 + (x / (R * lat0.to_radians().cos())).to_degrees();
    (lat, lon)
}

/// (lat, lon) -> local meters (x east, z south)
pub fn latlon_to_local(lat: f64, lon: f64) -> (f64, f64) {
    let (lat0, lon0) = anchor();
    let x = R * (lon - lon0).to_radians() * lat0.to_radians().cos();
    let z = -R * (lat - lat0).to_radians();
    (x, z)
}

/// CPU-baked terrain: heights, per-cell normals, and the square ground mesh.
pub struct TerrainBaker {
    pub pem: Pem,
}

impl TerrainBaker {
    pub fn new(pem: Pem) -> Self {
        TerrainBaker { pem }
    }

    /// bake normals for the whole square (cheap CPU port of the
    /// `heightmap/calc_normals` pass; recomputed after any bake)
    pub fn bake(&mut self) {
        self.pem.mark_clean();
    }

    /// world side length in meters
    pub fn side_m(&self) -> f64 {
        (self.pem.side_chunks as f64) * (RESOLUTION as f64 * self.cell_m())
    }

    pub fn cell_m(&self) -> f64 {
        512.0 / RESOLUTION as f64
    }

    /// Build the square ground patch as renderable MeshParts
    /// (positions/indices/uv/tex via the same shape the rasterizer consumes).
    /// Vertex color = grass/rock/sand mix by height + slope; tex -1 (flat).
    pub fn square_stage(&self) -> Vec<roads::mesh3d::MeshPart> {
        let n = (self.pem.side_chunks * RESOLUTION as u32) as usize; // cells per side
        let cell = self.cell_m();
        let x0 = self.pem.origin.0;
        let z0 = self.pem.origin.1;
        let side = self.side_m();
        let mut pos = Vec::with_capacity((n + 1) * (n + 1));
        let mut col = Vec::with_capacity((n + 1) * (n + 1));
        let mut idx = Vec::with_capacity(n * n * 6);
        for cz in 0..=n {
            for cx in 0..=n {
                let x = x0 + cx as f64 * cell;
                let z = z0 + cz as f64 * cell;
                let y = self.pem.height(x, z) as f64;
                pos.push(Vec3::new(x as f32, y as f32, z as f32));
                col.push(ground_color(&self.pem, x, z, y, side));
            }
        }
        for cz in 0..n {
            for cx in 0..n {
                let i0 = (cz * (n + 1) + cx) as u32;
                let i1 = i0 + 1;
                let i2 = i0 + (n as u32) + 1;
                let i3 = i2 + 1;
                idx.extend_from_slice(&[i0, i2, i1, i1, i2, i3]);
            }
        }
        let mut part = roads::mesh3d::MeshPart::new([1.0, 1.0, 1.0, 1.0]);
        part.positions = pos;
        part.indices = idx;
        // per-vertex colors via uv trick: MeshPart has a single tint, so bake
        // the palette into uv = (col_index, 0) rows of the GRASS texture.
        // Simpler & robust: 8 palette bands as flat sub-parts.
        part.uv = Vec::new();
        let mut parts = Vec::new();
        // split per color band so each gets a flat tint
        let bands: [[f32; 4]; 6] = [
            [0.44, 0.62, 0.30, 1.0], // low grass
            [0.47, 0.66, 0.32, 1.0], // grass
            [0.55, 0.62, 0.33, 1.0], // dry grass
            [0.62, 0.57, 0.40, 1.0], // dirt
            [0.55, 0.52, 0.47, 1.0], // rock
            [0.76, 0.72, 0.58, 1.0], // sand/road margin
        ];
        // bucket triangles by band index of centroid color
        let mut buckets: Vec<Vec<u32>> = vec![Vec::new(); bands.len()];
        for tri in part.indices.chunks(3) {
            let c0 = col[tri[0] as usize];
            // nearest band by luminance match is unnecessary — color IS the band id
            let bi = c0[3].mul_add(10.0, 0.0) as usize; // we encoded band in alpha
            buckets[bi.min(bands.len() - 1)].extend_from_slice(tri);
        }
        for (bi, tris) in buckets.iter().enumerate() {
            if tris.is_empty() {
                continue;
            }
            let mut p = roads::mesh3d::MeshPart::new(bands[bi]);
            p.positions = part.positions.clone();
            p.indices = tris.clone();
            parts.push(p);
        }
        if parts.is_empty() {
            parts.push(part);
        }
        parts
    }
}

/// per-vertex ground color: grass -> dry grass -> dirt -> rock by NORMALIZED
/// height (the Dar es Salaam plateau spans 40-90 m, so raw thresholds would
/// paint everything one band) plus slope. Band index smuggled through color
/// alpha so square_stage can bucket.
fn ground_color(pem: &Pem, x: f64, z: f64, y: f64, _side: f64) -> [f32; 4] {
    let h = y;
    let e = 2.0f64;
    let hx = pem.height(x + e, z) as f64;
    let hz = pem.height(x, z + e) as f64;
    let slope = ((hx - h).hypot(hz - h) / e).min(1.0);
    // normalized height across the square world (0..1)
    let side = pem.side_m();
    let (ox, oz) = pem.origin;
    let mut mn = f64::INFINITY;
    let mut mx = f64::NEG_INFINITY;
    let step = side / 24.0;
    let mut sx = ox + step * 0.5;
    while sx < ox + side {
        let mut sz = oz + step * 0.5;
        while sz < oz + side {
            let v = pem.height(sx, sz) as f64;
            mn = mn.min(v);
            mx = mx.max(v);
            sz += step;
        }
        sx += step;
    }
    let t = ((h - mn) / (mx - mn).max(1.0)).clamp(0.0, 1.0);
    let band: usize = if slope > 0.5 {
        4 // rock
    } else if t > 0.86 {
        5 // high sand
    } else if t > 0.68 {
        3 // dirt
    } else if t > 0.45 {
        2 // dry grass
    } else if t > 0.22 {
        1 // grass
    } else {
        0 // low grass
    };
    let rgb: [[f32; 3]; 6] = [
        [0.42, 0.60, 0.28],
        [0.47, 0.64, 0.31],
        [0.55, 0.61, 0.32],
        [0.60, 0.55, 0.38],
        [0.54, 0.51, 0.46],
        [0.74, 0.70, 0.56],
    ];
    let c = rgb[band];
    [c[0], c[1], c[2], band as f32 / 10.0]
}

/// height helper used by import glue: DEM grid -> packed chunk values
pub fn dem_to_packed(sample: impl Fn(f64, f64) -> Option<f32>, x: f64, z: f64) -> u16 {
    let (lat, lon) = local_to_latlon(x, z);
    sample(lat, lon).map(pack_height).unwrap_or(0)
}

pub use crate::pem::unpack_height as unpack;

/// the single square ground patch type (kept for API naming parity)
pub type SquareStage = TerrainBaker;
