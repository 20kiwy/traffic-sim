//! Mapbox tile providers + a hand-rolled Mapbox Vector Tile (MVT)
//! decoder (protobuf wire format parsed manually — no protobuf deps).
//!
//! Mapbox serves MVT gzip-encoded; [`decode_mvt`] sniffs the gzip magic
//! and inflates via the system `gzip` binary, so both raw wire bytes and
//! plain tiles decode.
//!
//! Tile math follows the standard slippy-map scheme (Web Mercator):
//! `z/x/y` tiles, 256 px nominal, MVT layers default to an extent of
//! 4096. Decoded geometry is returned as coordinates **normalized to
//! [0, 1] inside the tile** (x right, y down, tile space) — convert to
//! degrees with [`normalized_to_lonlat`] / [`tile_to_lonlat`] once the
//! tile's `z/x/y` are known.

use crate::osm::http_get;
use crate::types::SurfaceKind;
use serde_json::Value;
use std::io;
use std::path::Path;
use std::process::Command;

/// Re-export the tile-diagnostics module under `mapbox` as well
/// (`tsdata::mapbox::probe::describe_mvt` == `tsdata::probe::describe_mvt`).
pub use crate::probe;

/// Default MVT layer extent (Mapbox tiles use 4096).
pub const MVT_EXTENT: f64 = 4096.0;

/* ============================== tile math ============================== */

/// Web-Mercator normalized y in [0, 1] (0 = north pole edge) for a latitude.
pub fn mercator_norm(lat: f64) -> f64 {
    let lat = lat.clamp(-85.051_128_78, 85.051_128_78).to_radians();
    (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0
}

/// Inverse of [`mercator_norm`]: normalized y → latitude in degrees.
pub fn mercator_lat(y: f64) -> f64 {
    let n = std::f64::consts::PI * (1.0 - 2.0 * y);
    n.sinh().atan().to_degrees()
}

/// Slippy tile containing (lon, lat) at zoom `z` (clamped to the world).
pub fn lonlat_to_tile(lon: f64, lat: f64, z: u32) -> (u32, u32) {
    let n = (1u64 << z) as f64;
    let xf = ((lon + 180.0) / 360.0 * n).floor();
    let yf = (mercator_norm(lat) * n).floor();
    (
        (xf.max(0.0).min(n - 1.0)) as u32,
        (yf.max(0.0).min(n - 1.0)) as u32,
    )
}

/// Lon/lat degrees of a point inside tile `z/x/y`.
///
/// `px` / `py` are tile-local pixel coordinates in the 4096 extent
/// (y grows downward, matching MVT geometry space). For coordinates
/// normalized to [0, 1] (as produced by [`decode_mvt`]) use
/// [`normalized_to_lonlat`] or scale by [`MVT_EXTENT`] first.
pub fn tile_to_lonlat(z: u32, x: u32, y: u32, px: f64, py: f64) -> [f64; 2] {
    let n = (1u64 << z) as f64;
    let xf = (x as f64 + px / MVT_EXTENT) / n;
    let yf = (y as f64 + py / MVT_EXTENT) / n;
    let lon = xf * 360.0 - 180.0;
    let lat = mercator_lat(yf);
    [lon, lat]
}

/// Convert normalized tile-space coordinates ([0, 1], as stored in
/// [`MvtFeature::coords`]) to lon/lat degrees for the given slippy tile.
pub fn normalized_to_lonlat(z: u32, x: u32, y: u32, nx: f64, ny: f64) -> [f64; 2] {
    tile_to_lonlat(z, x, y, nx * MVT_EXTENT, ny * MVT_EXTENT)
}

/* ================================ URLs ================================= */

/// Mapbox Streets v8 vector tile URL (MVT).
pub fn vector_tile_url(z: u32, x: u32, y: u32) -> String {
    format!(
        "https://api.mapbox.com/v4/mapbox.mapbox-streets-v8/{z}/{x}/{y}.mvt?access_token={}",
        crate::token::mapbox_token()
    )
}

/// Mapbox Terrain v2 tile URL (`pngraw` = terrain-RGB encoding).
pub fn terrain_tile_url(z: u32, x: u32, y: u32) -> String {
    format!(
        "https://api.mapbox.com/v4/mapbox.mapbox-terrain-v2/{z}/{x}/{y}.pngraw?access_token={}",
        crate::token::mapbox_token()
    )
}

/// Mapbox Satellite raster tile URL (256 px PNG).
pub fn satellite_tile_url(z: u32, x: u32, y: u32) -> String {
    format!(
        "https://api.mapbox.com/v4/mapbox.satellite/{z}/{x}/{y}.png256?access_token={}",
        crate::token::mapbox_token()
    )
}

/// Fetch one streets-v8 MVT tile (cached as `streets_{z}_{x}_{y}.mvt`).
pub fn fetch_vector_tile(z: u32, x: u32, y: u32, cache_dir: &Path) -> io::Result<Vec<u8>> {
    http_get(
        &vector_tile_url(z, x, y),
        Some(&cache_dir.join(format!("streets_{z}_{x}_{y}.mvt"))),
    )
}

/// Fetch one terrain-v2 tile (cached as `terrain_{z}_{x}_{y}.pngraw`).
pub fn fetch_terrain_tile(z: u32, x: u32, y: u32, cache_dir: &Path) -> io::Result<Vec<u8>> {
    http_get(
        &terrain_tile_url(z, x, y),
        Some(&cache_dir.join(format!("terrain_{z}_{x}_{y}.pngraw"))),
    )
}

/// Fetch one satellite tile (cached as `satellite_{z}_{x}_{y}.png`).
pub fn fetch_satellite(z: u32, x: u32, y: u32, cache_dir: &Path) -> io::Result<Vec<u8>> {
    http_get(
        &satellite_tile_url(z, x, y),
        Some(&cache_dir.join(format!("satellite_{z}_{x}_{y}.png"))),
    )
}

/* ============================ MVT decoder ============================== */
//
// Minimal Google-protobuf wire-format reader:
//   key   = varint (field_number << 3 | wire_type)
//   wt 0  = varint, wt 1 = 64-bit fixed, wt 2 = length-delimited,
//   wt 5  = 32-bit fixed
//
// Tile message  : repeated field 3 = layers (messages); field 1 is
//                 defensively accepted as a layer too.
// Layer message : 1 = name (string), 3 = keys (strings),
//                 4 = values (messages), 2 / 5 = features + extent.
//                 The MVT 2.1 spec puts features on 2 and extent on 5;
//                 the wire types disambiguate (features are
//                 length-delimited, extent a varint) so BOTH layouts are
//                 accepted.
// Value message : 1 string, 2 float (fixed32), 3 double (fixed64),
//                 4 int64 (varint), 5 uint64 (varint), 6 sint64
//                 (zigzag varint), 7 bool.
// Feature       : 1 id (varint), 2 tags (packed varint key/value index
//                 pairs), 3 type (1 point / 2 line / 3 polygon),
//                 4 geometry (packed varint command stream).
// Geometry      : command integer `cmd = c & 7`, `count = c >> 3`;
//                 1 MoveTo (2 params), 2 LineTo (2 params), 3 Close (0);
//                 params are zigzag-encoded deltas in tile units.

/// One decoded MVT feature.
#[derive(Clone, Debug)]
pub struct MvtFeature {
    /// Layer name, e.g. `"road"`, `"bridge"`, `"building"`.
    pub layer: String,
    /// Decoded key/value tags.
    pub tags: serde_json::Map<String, Value>,
    /// Polylines / polygon rings as coordinates normalized to [0, 1]
    /// inside the tile (x right, y down). Polygon rings are NOT
    /// explicitly vertex-closed (the implicit last→first edge applies).
    pub coords: Vec<Vec<[f64; 2]>>,
}

/// A road-ish feature decoded from a streets-v8 tile (`road` / `bridge`
/// layers).
#[derive(Clone, Debug)]
pub struct MvtRoad {
    /// Merged polyline vertices, normalized tile space ([0, 1], y down).
    pub coords: Vec<[f64; 2]>,
    /// `class` tag: motorway | trunk | primary | secondary | tertiary |
    /// street | street_limited | service | path | ...
    pub class: String,
    /// `structure` tag: bridge | tunnel | fortified | none.
    pub structure: String,
    /// `surface` tag mapped through [`SurfaceKind::from_tag`].
    pub surface: SurfaceKind,
    /// `name` tag (may be empty).
    pub name: String,
    /// True when any tag KEY or tag VALUE contains "bus" (BRT heuristic:
    /// streets-v8 encodes bus-lane metadata on BRT corridors).
    pub brt: bool,
}

impl MvtRoad {
    /// Vertices in lon/lat degrees for the tile this feature came from.
    pub fn lonlat(&self, z: u32, x: u32, y: u32) -> Vec<[f64; 2]> {
        self.coords
            .iter()
            .map(|c| normalized_to_lonlat(z, x, y, c[0], c[1]))
            .collect()
    }
}

/* --------------------------- wire primitives --------------------------- */

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Reader { b, pos: 0 }
    }

    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..70).step_by(7) {
            let byte = *self.b.get(self.pos)?;
            self.pos += 1;
            v |= ((byte & 0x7f) as u64) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
            if shift >= 63 {
                break;
            }
        }
        None
    }

    fn key(&mut self) -> Option<(u32, u32)> {
        let k = self.varint()?;
        Some(((k >> 3) as u32, (k & 7) as u32))
    }

    fn len_bytes(&mut self) -> Option<&'a [u8]> {
        let l = self.varint()? as usize;
        let end = self.pos.checked_add(l)?;
        if end > self.b.len() {
            return None;
        }
        let s = &self.b[self.pos..end];
        self.pos = end;
        Some(s)
    }

    fn fixed32(&mut self) -> Option<f32> {
        let end = self.pos.checked_add(4)?;
        if end > self.b.len() {
            return None;
        }
        let mut a = [0u8; 4];
        a.copy_from_slice(&self.b[self.pos..end]);
        self.pos = end;
        Some(f32::from_bits(u32::from_le_bytes(a)))
    }

    fn fixed64(&mut self) -> Option<f64> {
        let end = self.pos.checked_add(8)?;
        if end > self.b.len() {
            return None;
        }
        let mut a = [0u8; 8];
        a.copy_from_slice(&self.b[self.pos..end]);
        self.pos = end;
        Some(f64::from_bits(u64::from_le_bytes(a)))
    }

    /// Skip a field of the given wire type. False when malformed.
    fn skip(&mut self, wt: u32) -> bool {
        match wt {
            0 => self.varint().is_some(),
            1 => {
                let end = match self.pos.checked_add(8) {
                    Some(e) => e,
                    None => return false,
                };
                if end > self.b.len() {
                    return false;
                }
                self.pos = end;
                true
            }
            2 => self.len_bytes().is_some(),
            5 => {
                let end = match self.pos.checked_add(4) {
                    Some(e) => e,
                    None => return false,
                };
                if end > self.b.len() {
                    return false;
                }
                self.pos = end;
                true
            }
            _ => false,
        }
    }
}

/// Zigzag-decode a varint into a signed value: `v>>1 ^ -(v&1)`.
fn zigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

fn utf8(b: &[u8]) -> Result<String, String> {
    std::str::from_utf8(b)
        .map(|s| s.to_string())
        .map_err(|_| "invalid utf8 in string field".to_string())
}

/* ------------------------------ decoding ------------------------------- */

/// Inflate a gzip stream via the system `gzip` binary (std has no TLS —
/// and no DEFLATE either; same sandbox pattern as the curl transport).
/// The input is staged through a temp file, output captured from stdout.
fn gunzip_bytes(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let tmp_in = std::env::temp_dir().join(format!("tsdata-mvt-{}.gz", crate::osm::unique_suffix()));
    std::fs::write(&tmp_in, bytes).map_err(|e| format!("gzip temp write: {e}"))?;
    let out = Command::new("gzip").arg("-dc").arg(&tmp_in).output();
    let _ = std::fs::remove_file(&tmp_in);
    match out {
        Ok(o) if o.status.success() => Ok(o.stdout),
        Ok(o) => Err(format!(
            "gzip -dc failed: {}",
            String::from_utf8_lossy(&o.stderr).trim()
        )),
        Err(e) => Err(format!("gzip spawn failed: {e}")),
    }
}

/// Decode a Mapbox Vector Tile into its features.
///
/// Mapbox serves MVT gzip-encoded and file caches may hold the raw
/// response bytes, so gzip payloads (magic `1f 8b`) are transparently
/// inflated first. Returns an error string when the protobuf stream is
/// malformed. Layers are decoded leniently: a malformed individual
/// feature is skipped.
pub fn decode_mvt(bytes: &[u8]) -> Result<Vec<MvtFeature>, String> {
    let data: &[u8] = if bytes.len() > 2 && bytes[0] == 0x1f && bytes[1] == 0x8b {
        &gunzip_bytes(bytes)?
    } else {
        bytes
    };
    let mut out = Vec::new();
    let mut r = Reader::new(data);
    while let Some((field, wt)) = r.key() {
        match (field, wt) {
            (3, 2) => {
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "tile: truncated layer".to_string())?;
                out.extend(parse_layer(sub)?);
            }
            (1, 2) => {
                // Defensive: accept a layer on field 1 as well (the wire
                // type check keeps a plain string field from breaking us;
                // layers without a name are rejected by parse_layer).
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "tile: truncated field 1".to_string())?;
                if let Ok(fs) = parse_layer(sub) {
                    out.extend(fs);
                }
            }
            _ => {
                if !r.skip(wt) {
                    return Err(format!("tile: malformed field {field} (wire type {wt})"));
                }
            }
        }
    }
    Ok(out)
}

struct RawFeature {
    tags: Vec<u64>,
    geom: Vec<u64>,
}

fn parse_layer(b: &[u8]) -> Result<Vec<MvtFeature>, String> {
    let mut name = String::new();
    let mut extent: u64 = 4096;
    let mut keys: Vec<String> = Vec::new();
    let mut values: Vec<Value> = Vec::new();
    let mut feats: Vec<RawFeature> = Vec::new();

    let mut r = Reader::new(b);
    while let Some((field, wt)) = r.key() {
        match (field, wt) {
            (1, 2) => {
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "layer: truncated name".to_string())?;
                name = utf8(sub)?;
            }
            (2, 2) => {
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "layer: truncated feature".to_string())?;
                if let Some(f) = parse_feature(sub) {
                    feats.push(f);
                }
            }
            (2, 0) => {
                // task-spec layout tolerance: extent on field 2 (varint)
                extent = r.varint().ok_or_else(|| "layer: truncated extent".to_string())?;
            }
            (3, 2) => {
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "layer: truncated key".to_string())?;
                keys.push(utf8(sub)?);
            }
            (4, 2) => {
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "layer: truncated value".to_string())?;
                if let Some(v) = parse_value(sub) {
                    values.push(v);
                }
            }
            (5, 0) => {
                // MVT 2.1 spec: extent = field 5 (varint)
                extent = r.varint().ok_or_else(|| "layer: truncated extent".to_string())?;
            }
            (5, 2) => {
                // task-spec layout tolerance: features on field 5
                let sub = r
                    .len_bytes()
                    .ok_or_else(|| "layer: truncated feature".to_string())?;
                if let Some(f) = parse_feature(sub) {
                    feats.push(f);
                }
            }
            (15, 0) => {
                // version — must be 15 for MVT; parsed and ignored
                r.varint().ok_or_else(|| "layer: truncated version".to_string())?;
            }
            _ => {
                if !r.skip(wt) {
                    return Err(format!(
                        "layer {name:?}: malformed field {field} (wire type {wt})"
                    ));
                }
            }
        }
    }

    if name.is_empty() {
        return Err("layer: missing name".to_string());
    }
    let extent_f = if extent == 0 { MVT_EXTENT } else { extent as f64 };

    Ok(feats
        .into_iter()
        .map(|f| MvtFeature {
            layer: name.clone(),
            tags: tag_map(&f.tags, &keys, &values),
            coords: decode_geometry(&f.geom, extent_f),
        })
        .collect())
}

fn parse_feature(b: &[u8]) -> Option<RawFeature> {
    let mut r = Reader::new(b);
    let mut tags = Vec::new();
    let mut geom = Vec::new();
    while let Some((field, wt)) = r.key() {
        match (field, wt) {
            (1, 0) => {
                r.varint()?; // feature id (unused)
            }
            (2, 2) => tags = packed_varints(r.len_bytes()?)?,
            (2, 0) => tags.push(r.varint()?),
            (3, 0) => {
                r.varint()?; // geometry type — inferred from commands
            }
            (4, 2) => geom = packed_varints(r.len_bytes()?)?,
            (4, 0) => geom.push(r.varint()?),
            _ => {
                if !r.skip(wt) {
                    return None;
                }
            }
        }
    }
    Some(RawFeature { tags, geom })
}

/// Decode a packed repeated-varint field body.
fn packed_varints(b: &[u8]) -> Option<Vec<u64>> {
    let mut r = Reader::new(b);
    let mut out = Vec::new();
    while r.pos < b.len() {
        out.push(r.varint()?);
    }
    Some(out)
}

fn parse_value(b: &[u8]) -> Option<Value> {
    let mut r = Reader::new(b);
    let mut v: Option<Value> = None;
    while let Some((field, wt)) = r.key() {
        match (field, wt) {
            (1, 2) => {
                let s = std::str::from_utf8(r.len_bytes()?).ok()?;
                v = Some(Value::String(s.to_string()));
            }
            (2, 5) => v = Some(serde_json::json!(r.fixed32()?)),
            (3, 1) => v = Some(serde_json::json!(r.fixed64()?)),
            (4, 0) => v = Some(serde_json::json!(r.varint()? as i64)),
            (5, 0) => v = Some(serde_json::json!(r.varint()?)),
            (6, 0) => v = Some(serde_json::json!(zigzag(r.varint()?))),
            (7, 0) => v = Some(serde_json::json!(r.varint()? != 0)),
            _ => {
                if !r.skip(wt) {
                    return None;
                }
            }
        }
    }
    v
}

/// Pair key/value indexes into a tag map.
fn tag_map(raw: &[u64], keys: &[String], values: &[Value]) -> serde_json::Map<String, Value> {
    let mut m = serde_json::Map::new();
    let mut i = 0;
    while i + 1 < raw.len() {
        let (ki, vi) = (raw[i] as usize, raw[i + 1] as usize);
        i += 2;
        if let (Some(k), Some(v)) = (keys.get(ki), values.get(vi)) {
            m.insert(k.clone(), v.clone());
        }
    }
    m
}

/// Decode an MVT geometry command stream into normalized rings.
///
/// Command integers: `cmd = c & 7`, `count = c >> 3`; 1 = MoveTo (2
/// params), 2 = LineTo (2 params), 3 = Close (0 params). Parameters are
/// zigzag varints, cumulative deltas in tile units.
fn decode_geometry(geom: &[u64], extent: f64) -> Vec<Vec<[f64; 2]>> {
    let e = if extent > 0.0 { extent } else { MVT_EXTENT };
    let norm = |x: i64, y: i64| -> [f64; 2] { [x as f64 / e, y as f64 / e] };
    let mut rings: Vec<Vec<[f64; 2]>> = Vec::new();
    let (mut x, mut y) = (0i64, 0i64);
    let mut i = 0usize;
    let mut cmd = 0u8;
    let mut count = 0u32;

    while i < geom.len() {
        if count == 0 {
            let c = geom[i];
            i += 1;
            cmd = (c & 0x7) as u8;
            count = (c >> 3) as u32;
            if cmd == 0 || cmd > 3 {
                break; // malformed command id
            }
        }
        match cmd {
            1 => {
                // MoveTo — starts a new polyline / ring
                if i + 1 >= geom.len() {
                    break;
                }
                x += zigzag(geom[i]);
                y += zigzag(geom[i + 1]);
                i += 2;
                rings.push(vec![norm(x, y)]);
                count -= 1;
            }
            2 => {
                // LineTo — extends the current polyline
                if i + 1 >= geom.len() {
                    break;
                }
                x += zigzag(geom[i]);
                y += zigzag(geom[i + 1]);
                i += 2;
                if let Some(last) = rings.last_mut() {
                    last.push(norm(x, y));
                }
                count -= 1;
            }
            3 => {
                // Close — polygon ring complete (no parameters)
                count -= 1;
            }
            _ => unreachable!(),
        }
    }
    rings.retain(|r| !r.is_empty());
    rings
}

/* ------------------------- roads from streets v8 ------------------------ */

fn tag_str<'a>(f: &'a MvtFeature, key: &str) -> Option<&'a str> {
    f.tags.get(key).and_then(|v| v.as_str())
}

/// True when any tag key or string tag value contains "bus"
/// (case-insensitive) — the streets-v8 BRT corridor heuristic.
fn tags_mention_bus(tags: &serde_json::Map<String, Value>) -> bool {
    tags.iter().any(|(k, v)| {
        k.to_ascii_lowercase().contains("bus")
            || v.as_str().map_or(false, |s| s.to_ascii_lowercase().contains("bus"))
    })
}

/// Extract road features from a streets-v8 tile: everything from the
/// `road` layer plus the `bridge` layer.
pub fn roads_from_mvt(bytes: &[u8]) -> Vec<MvtRoad> {
    let feats = match decode_mvt(bytes) {
        Ok(f) => f,
        Err(_) => return Vec::new(),
    };
    feats
        .into_iter()
        .filter(|f| f.layer == "road" || f.layer == "bridge")
        .map(|f| {
            // read all borrowed tag data BEFORE moving f.coords
            let class = tag_str(&f, "class").unwrap_or("unknown").to_string();
            let structure = tag_str(&f, "structure").unwrap_or("none").to_string();
            let surface = SurfaceKind::from_tag(tag_str(&f, "surface").unwrap_or(""));
            let name = tag_str(&f, "name").unwrap_or("").to_string();
            let brt = tags_mention_bus(&f.tags);
            MvtRoad {
                coords: f.coords.into_iter().flatten().collect(),
                class,
                structure,
                surface,
                name,
                brt,
            }
        })
        .collect()
}
