//! PEM — the Procedural Elevation Module: the DEM updater for traffic-sim.
//!
//! Ports the parts of egregoria that own ground height:
//! - `geom/src/heightmap.rs` packed heights (`pack_height`/`unpack_height`,
//!   u16, −39.97 m floor, 2048 m range) and 512 m / 32-cell chunks,
//! - `simulation/src/map/terrain.rs` terraform kinds
//!   (Elevation/Smooth/Level/Slope/Erode),
//! - `simulation/src/map/height_override.rs` — roads/bridges stamp their
//!   platform height into the cells they cross (`ROAD_Z_OFFSET` = 0.3 m).
//!
//! The upstream DEM (terrarium / Mapbox terrain-RGB, decoded in `tsdata`) is
//! pushed in through [`Pem::apply_dem`]; terraforming and road overrides then
//! edit on top, exactly like egregoria's pipeline.

pub use roads::surface;

/// chunk side in meters (egregoria `ChunkID_512`)
pub const CHUNK_SIZE: u32 = 512;
/// cells per chunk side (egregoria `TERRAIN_CHUNK_RESOLUTION`)
pub const RESOLUTION: usize = 32;
/// meters per cell
pub const CELL: f32 = CHUNK_SIZE as f32 / RESOLUTION as f32;
/// chunks per side of the square world
pub const CHUNKS_PER_SIDE: u32 = 4;
/// roads force terrain to `road_z - ROAD_Z_OFFSET`
pub const ROAD_Z_OFFSET: f32 = 0.3;

const MIN_HEIGHT: f32 = -39.97;
const HEIGHT_RANGE: f32 = 2048.0;

/// f32 meters <-> u16 packed (egregoria `pack_height`/`unpack_height`).
pub fn pack_height(h: f32) -> u16 {
    let t = ((h - MIN_HEIGHT) / HEIGHT_RANGE).clamp(0.0, 1.0);
    let v = (t * 65535.0).round() as u16;
    v
}

pub fn unpack_height(v: u16) -> f32 {
    let t = v as f32 / 65535.0;
    MIN_HEIGHT + t * HEIGHT_RANGE
}

/// Terraform tool kinds, copied from egregoria `TerraformKind`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum TerraformKind {
    Elevation,
    Smooth,
    Level,
    Slope,
    Erode,
}

impl TerraformKind {
    pub const ALL: [TerraformKind; 5] = [
        TerraformKind::Elevation,
        TerraformKind::Smooth,
        TerraformKind::Level,
        TerraformKind::Slope,
        TerraformKind::Erode,
    ];
    pub fn name(self) -> &'static str {
        match self {
            TerraformKind::Elevation => "Raise / Lower",
            TerraformKind::Smooth => "Smooth",
            TerraformKind::Level => "Level",
            TerraformKind::Slope => "Slope",
            TerraformKind::Erode => "Erode",
        }
    }
}

/// One 512 m chunk of packed heights, row-major, RESOLUTION² cells.
#[derive(Clone)]
pub struct Chunk {
    pub id: (u32, u32),
    pub heights: Vec<u16>,
    pub dirty: bool,
}

impl Chunk {
    fn new(id: (u32, u32)) -> Self {
        Chunk {
            id,
            heights: vec![pack_height(0.0); RESOLUTION * RESOLUTION],
            dirty: true,
        }
    }
    pub fn height_at(&self, cx: usize, cz: usize) -> f32 {
        unpack_height(self.heights[cz * RESOLUTION + cx])
    }
}

/// The PEM: a square world of chunks fed by DEM updates and tool edits.
pub struct Pem {
    pub chunks: Vec<Chunk>,
    /// world origin (chunk 0,0 corner) in local meters
    pub origin: (f64, f64),
    /// chunks per side (adaptive; CHUNKS_PER_SIDE is the default)
    pub side_chunks: u32,
    /// debug counter for suspicious packed writes
    pub debug_writes: usize,
}

impl Pem {
    pub fn new(origin: (f64, f64)) -> Self {
        Self::with_side(origin, CHUNKS_PER_SIDE)
    }

    /// Square world of `side_chunks`² chunks (each 512 m / 32 cells).
    pub fn with_side(origin: (f64, f64), side_chunks: u32) -> Self {
        let mut chunks = Vec::with_capacity((side_chunks * side_chunks) as usize);
        for cz in 0..side_chunks {
            for cx in 0..side_chunks {
                chunks.push(Chunk::new((cx, cz)));
            }
        }
        Pem { chunks, origin, side_chunks, debug_writes: 0 }
    }

    pub fn side_m(&self) -> f64 {
        self.side_chunks as f64 * CHUNK_SIZE as f64
    }

    fn chunk_mut(&mut self, cx: u32, cz: u32) -> Option<&mut Chunk> {
        let idx = (cz * self.side_chunks + cx) as usize;
        self.chunks.get_mut(idx)
    }

    /// raw (unpacked) cell write — non-finite guards (NaN would pack to 0
    /// and paint a −40 m hole in the world)
    pub fn set_cell(&mut self, x_m: f64, z_m: f64, h_m: f32) {
        if !h_m.is_finite() || !x_m.is_finite() || !z_m.is_finite() {
            return;
        }
        let fx = (x_m - self.origin.0) / CHUNK_SIZE as f64;
        let fz = (z_m - self.origin.1) / CHUNK_SIZE as f64;
        let (cx, cz) = (fx.floor() as u32, fz.floor() as u32);
        if cx >= self.side_chunks || cz >= self.side_chunks {
            return;
        }
        let lx = ((fx - cx as f64) * RESOLUTION as f64).floor() as usize;
        let lz = ((fz - cz as f64) * RESOLUTION as f64).floor() as usize;
        let packed = pack_height(h_m);
        let dbg = (packed == 0 || packed == u16::MAX) && self.debug_writes < 8;
        if dbg {
            self.debug_writes += 1;
            eprintln!(
                "[pem-debug] suspicious write h={h_m} at ({x_m:.0},{z_m:.0}) -> packed {packed}"
            );
        }
        if let Some(c) = self.chunk_mut(cx, cz) {
            let i = (lz.min(RESOLUTION - 1)) * RESOLUTION + (lx.min(RESOLUTION - 1));
            c.heights[i] = packed;
            c.dirty = true;
        }
    }

    /// bilinear ground height in local meters
    pub fn height(&self, x_m: f64, z_m: f64) -> f32 {
        let fx = ((x_m - self.origin.0) / CELL as f64).max(0.0);
        let fz = ((z_m - self.origin.1) / CELL as f64).max(0.0);
        let side = ((self.side_chunks as usize) * RESOLUTION) as f64 - 1.0;
        let fx = fx.min(side);
        let fz = fz.min(side);
        let x0 = fx.floor() as usize;
        let z0 = fz.floor() as usize;
        let tx = (fx - x0 as f64) as f32;
        let tz = (fz - z0 as f64) as f32;
        let at = |x: usize, z: usize| -> f32 {
            let ccx = (x / RESOLUTION) as u32;
            let ccz = (z / RESOLUTION) as u32;
            if ccx >= self.side_chunks || ccz >= self.side_chunks {
                return 0.0;
            }
            self.chunks[(ccz * self.side_chunks + ccx) as usize]
                .height_at(x % RESOLUTION, z % RESOLUTION)
        };
        let h = at(x0, z0) * (1.0 - tx) * (1.0 - tz)
            + at(x0 + 1, z0) * tx * (1.0 - tz)
            + at(x0, z0 + 1) * (1.0 - tx) * tz
            + at(x0 + 1, z0 + 1) * tx * tz;
        h
    }

    /// DEM update: sample an external DEM (lat/lon → meters) into every cell.
    /// `sample(lat, lon) -> Option<elev_m>`; unanswerable cells keep old value.
    pub fn apply_dem(&mut self, mut sample: impl FnMut(f64, f64) -> Option<f32>) {
        let side = ((self.side_chunks as usize) * RESOLUTION) as u32;
        for cz in 0..side {
            for cx in 0..side {
                // cell center in local meters
                let x_m = self.origin.0 + (cx as f64 + 0.5) * CELL as f64;
                let z_m = self.origin.1 + (cz as f64 + 0.5) * CELL as f64;
                let (lat, lon) = crate::terrain::local_to_latlon(x_m, z_m);
                if let Some(h) = sample(lat, lon) {
                    self.set_cell(x_m, z_m, h);
                }
            }
        }
    }

    /// Terraform edit (egregoria `Environment::terraform` semantics, simplified:
    /// gaussian falloff, radius in meters, amount in meters).
    pub fn terraform(&mut self, kind: TerraformKind, center: (f64, f64), radius: f64, amount: f32) {
        let (cx, cz) = center;
        let r2 = (radius * radius) as f64;
        let x0 = cx - radius;
        let x1 = cx + radius;
        let z0 = cz - radius;
        let z1 = cz + radius;
        // gather target level once for Level/Slope
        let level = self.height(cx, cz);
        let step = CELL as f64;
        let mut z = z0;
        while z <= z1 {
            let mut x = x0;
            while x <= x1 {
                let dx = x - cx;
                let dz = z - cz;
                let d2 = dx * dx + dz * dz;
                if d2 <= r2 {
                    let t = 1.0 - (d2 / r2).sqrt();
                    let w = t * t * (3.0 - 2.0 * t); // smoothstep
                    let h0 = self.height(x, z);
                    let h = match kind {
                        TerraformKind::Elevation => h0 + amount * w as f32,
                        TerraformKind::Smooth => h0 * (1.0 - w as f32) + level * w as f32,
                        TerraformKind::Level => h0 * (1.0 - w as f32) + level * w as f32,
                        TerraformKind::Slope => h0 + (amount * w as f32 - h0) * w as f32,
                        TerraformKind::Erode => h0 - (h0 - level).abs() * 0.15 * w as f32,
                    };
                    self.set_cell(x, z, h);
                }
                x += step;
            }
            z += step;
        }
    }

    /// Road/bridge platform override — port of egregoria
    /// `height_override::find_overrides` for roads: every cell within
    /// `10 + width*3` meters of the polyline gets stamped to
    /// `road_elev - ROAD_Z_OFFSET` (min-combined).
    pub fn stamp_roads(&mut self, pts: &[(f64, f64, f64)], width_m: f64) {
        if pts.len() < 2 {
            return;
        }
        let margin = 10.0 + width_m * 3.0;
        for w in pts.windows(2) {
            let (x0, z0, y0) = w[0];
            let (x1, z1, y1) = w[1];
            let len = ((x1 - x0).hypot(z1 - z0)).max(0.001);
            let steps = ((len / (CELL as f64 * 0.5)).ceil() as usize).max(1);
            for i in 0..=steps {
                let t = i as f64 / steps as f64;
                let x = x0 + (x1 - x0) * t;
                let z = z0 + (z1 - z0) * t;
                let y = y0 + (y1 - y0) * t;
                let r = margin;
                let step = CELL as f64;
                let mut dz = -r;
                while dz <= r {
                    let mut dx = -r;
                    while dx <= r {
                        if dx * dx + dz * dz <= r * r {
                            let px = x + dx;
                            let pz = z + dz;
                            let cur = self.height(px, pz);
                            let target = y as f32 - ROAD_Z_OFFSET;
                            if target < cur || (y - cur as f64).abs() > ROAD_Z_OFFSET as f64 {
                                self.set_cell(px, pz, target);
                            }
                        }
                        dx += step;
                    }
                    dz += step;
                }
            }
        }
    }

    /// count of chunks needing a rebake
    pub fn dirty_chunks(&self) -> usize {
        self.chunks.iter().filter(|c| c.dirty).count()
    }

    pub fn mark_clean(&mut self) {
        for c in &mut self.chunks {
            c.dirty = false;
        }
    }
}

/// `Heightmap` alias matching the report vocabulary.
pub type Heightmap = Pem;
