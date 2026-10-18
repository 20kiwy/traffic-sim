//! DEM (digital elevation model) tiles: Terrarium (AWS Terrain Tiles,
//! no key) and Mapbox Terrain-RGB decoding + bilinear sampling.
//!
//! Both encodings are PNG RGBA rasters where elevation is packed into
//! R/G/B:
//! - Terrarium: `elev_m = (R*256 + G + B/256) - 32768`
//! - Terrain-RGB: `elev_m = -10000 + (R*65536 + G*256 + B) * 0.1`
//!
//! A [`DemGrid`] covers ONE 256×256 tile (or an exact multiple when
//! tiles are stitched into a mosaic by [`fetch_dem_for`]).

use crate::mapbox::{mercator_lat, mercator_norm};
use crate::types::GeoBounds;
use std::io;
use std::path::Path;

/// Elevation grid for a single slippy-map tile (or a stitched mosaic).
#[derive(Clone, Debug)]
pub struct DemGrid {
    /// Grid width in pixels.
    pub w: usize,
    /// Grid height in pixels.
    pub h: usize,
    /// Zoom level. 0 until the caller assigns the tile identity.
    pub z: u32,
    /// Slippy tile x (leftmost for mosaics).
    pub tx: u32,
    /// Slippy tile y (topmost for mosaics).
    pub ty: u32,
    /// Heights in meters, row-major (`y * w + x`).
    pub heights: Vec<f32>,
}

impl DemGrid {
    /// Decode a Terrarium PNG (AWS `elevation-tiles-prod`).
    pub fn from_terrarium_png(bytes: &[u8]) -> Result<Self, String> {
        Self::from_terrarium_png_tile(bytes, 0, 0, 0)
    }

    /// Terrarium tile decode with the slippy tile coords — REQUIRED for
    /// `sample`/`tile_box` to map (lat, lon) onto pixels correctly.
    pub fn from_terrarium_png_tile(bytes: &[u8], z: u32, tx: u32, ty: u32) -> Result<Self, String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("terrarium png: {e}"))?
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let mut heights = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let px = img.get_pixel(x as u32, y as u32);
                let [r, g, b, _] = px.0;
                heights.push((r as f32) * 256.0 + (g as f32) + (b as f32) / 256.0 - 32768.0);
            }
        }
        Ok(Self { w, h, z, tx, ty, heights })
    }

    /// Decode a Mapbox Terrain-RGB `pngraw` tile.
    pub fn from_mapbox_terrain_rgb(bytes: &[u8]) -> Result<Self, String> {
        Self::from_mapbox_terrain_rgb_tile(bytes, 0, 0, 0)
    }

    pub fn from_mapbox_terrain_rgb_tile(bytes: &[u8], z: u32, tx: u32, ty: u32) -> Result<Self, String> {
        let img = image::load_from_memory(bytes)
            .map_err(|e| format!("terrain-rgb png: {e}"))?
            .to_rgba8();
        let (w, h) = (img.width() as usize, img.height() as usize);
        let mut heights = Vec::with_capacity(w * h);
        for y in 0..h {
            for x in 0..w {
                let px = img.get_pixel(x as u32, y as u32);
                let [r, g, b, _] = px.0;
                heights.push(
                    -10000.0 + ((r as f32) * 65536.0 + (g as f32) * 256.0 + (b as f32)) * 0.1,
                );
            }
        }
        Ok(Self { w, h, z, tx, ty, heights })
    }

    /// Tiles covered by the grid (mosaics are exact multiples of 256 px;
    /// anything else is treated as a single tile).
    fn tile_span(&self) -> (u32, u32) {
        if self.w >= 256 && self.h >= 256 && self.w % 256 == 0 && self.h % 256 == 0 {
            ((self.w / 256) as u32, (self.h / 256) as u32)
        } else {
            (1, 1)
        }
    }

    /// Geographic box of the grid as `(west, south, east, north)` in
    /// degrees. For a single-tile grid this is the tile's own box; for a
    /// mosaic it spans every stitched tile.
    pub fn tile_box(&self) -> (f64, f64, f64, f64) {
        let n = (1u64 << self.z) as f64;
        let (cols, rows) = self.tile_span();
        let west = self.tx as f64 / n * 360.0 - 180.0;
        let east = (self.tx as f64 + cols as f64) / n * 360.0 - 180.0;
        let north = mercator_lat(self.ty as f64 / n);
        let south = mercator_lat((self.ty as f64 + rows as f64) / n);
        (west, south, east, north)
    }

    /// Bilinear sample at (lat, lon) degrees; `None` outside the grid.
    pub fn sample(&self, lat: f64, lon: f64) -> Option<f32> {
        if self.w == 0 || self.h == 0 || self.heights.len() < self.w * self.h {
            return None;
        }
        let (west, south, east, north) = self.tile_box();
        if !(west..=east).contains(&lon) || !(south..=north).contains(&lat) {
            return None;
        }
        let fx = (lon - west) / (east - west);
        let (yn, yn0, yn1) = (mercator_norm(lat), mercator_norm(north), mercator_norm(south));
        let fy = (yn - yn0) / (yn1 - yn0);
        let px = (fx * (self.w - 1) as f64).clamp(0.0, (self.w - 1) as f64);
        let py = (fy * (self.h - 1) as f64).clamp(0.0, (self.h - 1) as f64);

        let x0 = px.floor() as usize;
        let y0 = py.floor() as usize;
        let x1 = (x0 + 1).min(self.w - 1);
        let y1 = (y0 + 1).min(self.h - 1);
        let tx = (px - x0 as f64) as f32;
        let ty = (py - y0 as f64) as f32;

        let h00 = self.heights[y0 * self.w + x0];
        let h10 = self.heights[y0 * self.w + x1];
        let h01 = self.heights[y1 * self.w + x0];
        let h11 = self.heights[y1 * self.w + x1];
        let top = h00 + (h10 - h00) * tx;
        let bot = h01 + (h11 - h01) * tx;
        Some(top + (bot - top) * ty)
    }
}

/// Terrarium tile URL (AWS Terrain Tiles — no API key required).
pub fn terrarium_url(z: u32, x: u32, y: u32) -> String {
    format!("https://s3.amazonaws.com/elevation-tiles-prod/terrarium/{z}/{x}/{y}.png")
}

/// Fetch Terrarium DEM tile(s) covering `bounds` at zoom `z` and stitch
/// them into one grid (single tile when the bounds fit in one).
///
/// Tiles are cached as `terrarium_{z}_{x}_{y}.png` inside `cache_dir`.
pub fn fetch_dem_for(bounds: &GeoBounds, z: u32, cache_dir: &Path) -> io::Result<DemGrid> {
    let n = 1u64 << z;
    let clamp = |v: f64| -> u32 { (v.floor() as i64).clamp(0, n as i64 - 1) as u32 };
    let tx0 = clamp((bounds.west + 180.0) / 360.0 * n as f64);
    let tx1 = clamp((bounds.east + 180.0) / 360.0 * n as f64);
    let ty0 = clamp(mercator_norm(bounds.north) * n as f64);
    let ty1 = clamp(mercator_norm(bounds.south) * n as f64);
    let cols = tx1 - tx0 + 1;
    let rows = ty1 - ty0 + 1;

    let mut grids = Vec::with_capacity((cols * rows) as usize);
    for ty in ty0..=ty1 {
        for tx in tx0..=tx1 {
            let cache = cache_dir.join(format!("terrarium_{z}_{tx}_{ty}.png"));
            let bytes = crate::osm::http_get(&terrarium_url(z, tx, ty), Some(&cache))?;
            let g = DemGrid::from_terrarium_png(&bytes).map_err(|e| {
                io::Error::new(io::ErrorKind::InvalidData, format!("terrarium {z}/{tx}/{ty}: {e}"))
            })?;
            grids.push(g);
        }
    }

    if cols == 1 && rows == 1 {
        // cols*rows >= 1, so the list is non-empty here
        let mut g = grids.remove(0);
        g.z = z;
        g.tx = tx0;
        g.ty = ty0;
        return Ok(g);
    }

    // Stitch row-major into one mosaic.
    let tw = grids[0].w;
    let th = grids[0].h;
    if grids.iter().any(|g| g.w != tw || g.h != th) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "terrarium tiles with mixed sizes cannot be stitched",
        ));
    }
    let mut heights = vec![0.0f32; tw * cols as usize * th * rows as usize];
    for (k, g) in grids.iter().enumerate() {
        let col = (k as u32 % cols) as usize;
        let row = (k as u32 / cols) as usize;
        for gy in 0..th {
            let dst = (row * th + gy) * (tw * cols as usize) + col * tw;
            heights[dst..dst + tw].copy_from_slice(&g.heights[gy * tw..(gy + 1) * tw]);
        }
    }
    Ok(DemGrid { w: tw * cols as usize, h: th * rows as usize, z, tx: tx0, ty: ty0, heights })
}
