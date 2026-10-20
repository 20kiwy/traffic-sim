//! Texture assets (`.raw` RKTX files: magic 'RKTX' + u16 w + u16 h + rgba8).
//!
//! The road trim sheet is FETCHED from godot-road-generator
//! (`resources/road_texture@2x.png` → road_trim.raw), the rest are
//! generated procedural textures (grass / sidewalk / median / asphalt,
//! plus the R7 set: cement / mud / rough / red_tactile — regenerated via
//! `assets/textures_src/gen_cement_mud_rough.py`).
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
}

impl Texture {
    /// Parse an in-memory RKTX payload (magic 'RKTX' + u16 w + u16 h + rgba8).
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
        Ok(Self { w, h, data: bytes[8..8 + need].to_vec() })
    }

    pub fn load_rktx(path: &Path) -> std::io::Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_rktx_bytes(&bytes).map_err(|e| {
            std::io::Error::new(e.kind(), format!("{}: {}", path.display(), e))
        })
    }

    /// Bilinear RGBA sample with wrapped UVs (0..1, repeat).
    pub fn sample(&self, u: f32, v: f32) -> [f32; 4] {
        let w = self.w.max(1);
        let h = self.h.max(1);
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
                self.data[o] as f32 / 255.0,
                self.data[o + 1] as f32 / 255.0,
                self.data[o + 2] as f32 / 255.0,
                self.data[o + 3] as f32 / 255.0,
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
                    lib.entries.push(Texture { w: 1, h: 1, data: vec![128, 128, 128, 255] });
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
        const NAMES: [&str; 17] = [
            "road_trim", "grass", "sidewalk", "median", "asphalt",
            "cement", "mud", "rough", "red_tactile",
            // RoadArchitect set (R11)
            "ra_asphalt", "ra_concrete", "ra_galv", "ra_wbeam",
            "ra_railing", "ra_brick", "ra_cobble", "windows",
        ];
        const BYTES: [&[u8]; 17] = [
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
        ];
        let mut lib = Self::default();
        for (name, bytes) in NAMES.iter().zip(BYTES.iter()) {
            lib.index.insert(name.to_string(), lib.entries.len());
            match Texture::from_rktx_bytes(bytes) {
                Ok(t) => lib.entries.push(t),
                Err(_) => lib
                    .entries
                    .push(Texture { w: 1, h: 1, data: vec![128, 128, 128, 255] }),
            }
        }
        lib
    }

    /// Number of texture slots (loaded + fallbacks), for the UI.
    pub fn slot_count(&self) -> usize {
        self.entries.len()
    }
}
