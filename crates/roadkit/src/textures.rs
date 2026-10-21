//! Texture assets (`.raw` RKTX files: magic 'RKTX' + u16 w + u16 h + rgba8).
//!
//! The road trim sheet is FETCHED from godot-road-generator
//! (`resources/road_texture@2x.png` → road_trim.raw), the rest are
//! generated procedural textures (grass / sidewalk / median / asphalt,
//! the R7 set: cement / mud / rough / red_tactile via
//! `assets/textures_src/gen_cement_mud_rough.py`, and the R15 environment
//! set: terrain_grass / wavy via `assets/textures_src/gen_env_textures.py`).
//! Decoding PNGs at runtime would need an inflate crate; the conversion
//! happens once at build time instead.

use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug)]
pub struct Texture {
    pub w: usize,
    pub h: usize,
    /// rgba8
    pub data: Vec<u8>,
    /// box-filtered mip pyramid (level 1.., each half the previous size).
    /// Built once at load; the raster picks a level per triangle so distant
    /// surfaces sample averaged texels instead of shimming noise.
    pub mips: Vec<Mip>,
}

/// One downsampled level (level = 1 is half resolution, 2 is quarter…).
#[derive(Clone, Debug)]
pub struct Mip {
    pub w: usize,
    pub h: usize,
    pub data: Vec<u8>,
}

impl Texture {
    /// Build the mip pyramid (2×2 box filter per step). 1×1 textures get none.
    fn build_mips(mut self) -> Self {
        let mut w = self.w;
        let mut h = self.h;
        let mut data = self.data.clone();
        while w.min(h) > 1 {
            let (nw, nh) = (w / 2, h / 2);
            let mut out = vec![0u8; nw * nh * 4];
            for y in 0..nh {
                for x in 0..nw {
                    let (x2, y2) = (x * 2, y * 2);
                    for c in 0..4 {
                        let o = |xx: usize, yy: usize| ((yy * w + xx) * 4 + c) as usize;
                        let a = data[o(x2, y2)] as u32;
                        let b = data[o(x2 + 1, y2)] as u32;
                        let e = data[o(x2, y2 + 1)] as u32;
                        let d = data[o(x2 + 1, y2 + 1)] as u32;
                        out[(y * nw + x) * 4 + c] = ((a + b + e + d) / 4) as u8;
                    }
                }
            }
            self.mips.push(Mip { w: nw, h: nh, data: out.clone() });
            data = out;
            w = nw;
            h = nh;
        }
        self
    }

    /// Sample mip `level` (0 = the base texture), bilinear + wrap.
    pub fn sample_lod(&self, u: f32, v: f32, level: usize) -> [f32; 4] {
        if level > 0 {
            if let Some(m) = self.mips.get(level - 1) {
                return sample_buf(m.w, m.h, &m.data, u, v);
            }
        }
        sample_buf(self.w, self.h, &self.data, u, v)
    }
}

/// Bilinear RGBA sample from a raw pixel buffer with wrapped UVs (0..1, repeat).
fn sample_buf(w: usize, h: usize, data: &[u8], u: f32, v: f32) -> [f32; 4] {
    let w = w.max(1);
    let h = h.max(1);
    let x = u * w as f32 - 0.5;
    let y = v * h as f32 - 0.5;
    let x0 = x.floor();
    let y0 = y.floor();
    let tx = x - x0;
    let ty = y - y0;
    let wrap = |i: i64, n: usize| ((i % n as i64) + n as i64) as usize % n;
    let (ix0, iy0) = (wrap(x0 as i64, w), wrap(y0 as i64, h));
    let (ix1, iy1) = (wrap(x0 as i64 + 1, w), wrap(y0 as i64 + 1, h));
    let at = |ix: usize, iy: usize| -> [f32; 4] {
        let o = (iy * w + ix) * 4;
        [
            data[o] as f32 / 255.0,
            data[o + 1] as f32 / 255.0,
            data[o + 2] as f32 / 255.0,
            data[o + 3] as f32 / 255.0,
        ]
    };
    let c00 = at(ix0, iy0);
    let c10 = at(ix1, iy0);
    let c01 = at(ix0, iy1);
    let c11 = at(ix1, iy1);
    let mut out = [0.0f32; 4];
    for k in 0..4 {
        let a = c00[k] * (1.0 - tx) + c10[k] * tx;
        let b = c01[k] * (1.0 - tx) + c11[k] * tx;
        out[k] = a * (1.0 - ty) + b * ty;
    }
    out
}

impl Texture {
    pub fn from_rktx_bytes(bytes: &[u8]) -> std::io::Result<Self> {
        if bytes.len() < 8 || &bytes[0..4] != b"RKTX" {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "not an RKTX texture".to_string(),
            ));
        }
        let w = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        let h = u16::from_le_bytes([bytes[6], bytes[7]]) as usize;
        let need = w * h * 4;
        if bytes.len() < 8 + need {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("truncated RKTX (need {need} bytes)"),
            ));
        }
        Ok(Self { w, h, data: bytes[8..8 + need].to_vec(), mips: Vec::new() })
    }

    pub fn load_rktx(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_rktx_bytes(&bytes).map(|t| t.build_mips()).map_err(|e| {
            std::io::Error::new(e.kind(), format!("{}: {}", path.display(), e))
        })
    }

    /// Bilinear RGBA sample with wrapped UVs (0..1, repeat).
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        sample_buf(self.w, self.h, &self.data, u, v)
    }
}

/// Fixed slot order — indices are baked into mesh parts.
pub const TEX_ROAD_TRIM: i32 = 0;
pub const TEX_GRASS: i32 = 1;
pub const TEX_SIDEWALK: i32 = 2;
pub const TEX_MEDIAN: i32 = 3;
pub const TEX_ASPHALT: i32 = 4;
pub const TEX_CEMENT: i32 = 5;
pub const TEX_MUD: i32 = 6;
pub const TEX_ROUGH: i32 = 7;
pub const TEX_TACTILE: i32 = 8;
// RoadArchitect texture slots (converted by assets/textures_src/gen_ra_textures.py)
pub const TEX_RA_ASPHALT: i32 = 9;
pub const TEX_RA_CONCRETE: i32 = 10;
pub const TEX_RA_GALV: i32 = 11;
pub const TEX_RA_WBEAM: i32 = 12;
pub const TEX_RA_RAILING: i32 = 13;
pub const TEX_RA_BRICK: i32 = 14;
pub const TEX_RA_COBBLE: i32 = 15;
pub const TEX_WINDOWS: i32 = 16;
pub const TEX_RA_CONCRETE2: i32 = 17;
pub const TEX_RA_CONCRETE3: i32 = 18;
pub const TEX_RA_DIRT: i32 = 19;
// R15 environment set (converted by assets/textures_src/gen_env_textures.py).
// NOTE: TEX_GRASS (1) is the R6 roadside grass baked into road/junction mesh
// parts — the terrain ground albedo gets its own slot and name.
pub const TEX_TERRAIN_GRASS: i32 = 20;
pub const TEX_WAVY: i32 = 21;

#[derive(Default)]
pub struct TextureLibrary {
    pub entries: Vec<Texture>,
    index: HashMap<String, usize>,
}

impl TextureLibrary {
    /// Load the standard set from `<assets>/textures/*.raw`.
    pub fn load_dir(dir: &Path) -> Self {
        let names = [
            "road_trim", "grass", "sidewalk", "median", "asphalt",
            "cement", "mud", "rough", "red_tactile",
            // RoadArchitect set (R11)
            "ra_asphalt", "ra_concrete", "ra_galv", "ra_wbeam",
            "ra_railing", "ra_brick", "ra_cobble", "windows",
            // RoadArchitect set (R12)
            "ra_concrete2", "ra_concrete3", "ra_dirt",
            // R15 environment set (terrain ground + water noise)
            "terrain_grass", "wavy",
        ];
        let mut lib = Self::default();
        for name in names {
            let path = dir.join(format!("{name}.raw"));
            match Texture::load_rktx(&path) {
                Ok(t) => {
                    lib.index.insert(name.to_string(), lib.entries.len());
                    lib.entries.push(t);
                }
                Err(e) => {
                    eprintln!("[roadkit] texture {name}: {e}");
                    lib.index.insert(name.to_string(), lib.entries.len());
                    // 1x1 fallback so indices stay valid
                    lib.entries.push(Texture { w: 1, h: 1, data: vec![128, 128, 128, 255], mips: Vec::new() });
                }
            }
        }
        lib
    }

    pub fn get(&self, slot: i32) -> Option<&Texture> {
        if slot < 0 {
            return None;
        }
        self.entries.get(slot as usize)
    }

    /// The standard texture set embedded in the binary (the wasm32 browser
    /// build has no filesystem). Names + order MUST match `load_dir` so the
    /// road mesh texture slots stay valid.
    #[cfg(target_arch = "wasm32")]
    pub fn load_embedded() -> Self {
        const NAMES: [&str; 22] = [
            "road_trim", "grass", "sidewalk", "median", "asphalt",
            "cement", "mud", "rough", "red_tactile",
            // RoadArchitect set (R11)
            "ra_asphalt", "ra_concrete", "ra_galv", "ra_wbeam",
            "ra_railing", "ra_brick", "ra_cobble", "windows",
            // RoadArchitect set (R12)
            "ra_concrete2", "ra_concrete3", "ra_dirt",
            // R15 environment set (terrain ground + water noise)
            "terrain_grass", "wavy",
        ];
        const BYTES: [&[u8]; 22] = [
            include_bytes!("../../../assets/textures/road_trim.raw"),
            include_bytes!("../../../assets/textures/grass.raw"),
            include_bytes!("../../../assets/textures/sidewalk.raw"),
            include_bytes!("../../../assets/textures/median.raw"),
            include_bytes!("../../../assets/textures/asphalt.raw"),
            include_bytes!("../../../assets/textures/cement.raw"),
            include_bytes!("../../../assets/textures/mud.raw"),
            include_bytes!("../../../assets/textures/rough.raw"),
            include_bytes!("../../../assets/textures/red_tactile.raw"),
            include_bytes!("../../../assets/textures/ra_asphalt.raw"),
            include_bytes!("../../../assets/textures/ra_concrete.raw"),
            include_bytes!("../../../assets/textures/ra_galv.raw"),
            include_bytes!("../../../assets/textures/ra_wbeam.raw"),
            include_bytes!("../../../assets/textures/ra_railing.raw"),
            include_bytes!("../../../assets/textures/ra_brick.raw"),
            include_bytes!("../../../assets/textures/ra_cobble.raw"),
            include_bytes!("../../../assets/textures/windows.raw"),
            include_bytes!("../../../assets/textures/ra_concrete2.raw"),
            include_bytes!("../../../assets/textures/ra_concrete3.raw"),
            include_bytes!("../../../assets/textures/ra_dirt.raw"),
            include_bytes!("../../../assets/textures/terrain_grass.raw"),
            include_bytes!("../../../assets/textures/wavy.raw"),
        ];
        let mut lib = Self::default();
        for (name, bytes) in NAMES.iter().zip(BYTES.iter()) {
            lib.index.insert(name.to_string(), lib.entries.len());
            match Texture::from_rktx_bytes(bytes) {
                Ok(t) => lib.entries.push(t.build_mips()),
                Err(_) => lib
                    .entries
                    .push(Texture { w: 1, h: 1, data: vec![128, 128, 128, 255], mips: Vec::new() }),
            }
        }
        lib
    }

    /// Number of texture slots (loaded + fallbacks), for the UI.
    pub fn slot_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed slot set (road_trim .. TEX_WAVY) must load completely from
    /// the repo's assets dir — a missing/malformed .raw silently degrades to
    /// a 1x1 gray fallback, so assert real dimensions here.
    #[test]
    fn standard_set_loads_all_22_slots() {
        let lib = TextureLibrary::load_dir(Path::new("../../assets/textures"));
        assert_eq!(lib.slot_count(), 22, "TEX_ROAD_TRIM(0)..TEX_WAVY(21)");
        for slot in 0..22 {
            let t = lib.get(slot).unwrap_or_else(|| panic!("slot {slot} missing"));
            assert!(t.w > 1 && t.h > 1, "slot {slot} fell back to 1x1");
            assert!(!t.mips.is_empty(), "slot {slot} has no mip pyramid");
        }
        // R15 environment set specifics
        let grass = lib.get(TEX_TERRAIN_GRASS).unwrap();
        assert_eq!((grass.w, grass.h), (256, 256), "terrain_grass.raw");
        let wavy = lib.get(TEX_WAVY).unwrap();
        assert_eq!((wavy.w, wavy.h), (128, 128), "wavy.raw");
        // wavy must be mid-gray centered (~0.5 per channel) so sampling RGB
        // yields zero-mean offsets for water normal perturbation
        let mut mean = [0.0f64; 3];
        for px in wavy.data.chunks_exact(4) {
            for c in 0..3 {
                mean[c] += px[c] as f64;
            }
        }
        let n = (wavy.w * wavy.h) as f64;
        for c in 0..3 {
            let m = mean[c] / n / 255.0;
            assert!((m - 0.5).abs() < 0.02, "wavy channel {c} mean {m:.3} != ~0.5");
        }
    }
}
