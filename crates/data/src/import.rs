//! Import OSM JSON documents into the `roads` crate model (GRG
//! RoadPoint / RoadCurve in LOCAL METERS), plus offline demo entry
//! points for the Kijazi bridge and Igesa × Sam Nujoma areas.
//!
//! Coordinate convention (matching `roads`): **x east, z south**, origin
//! at the document's first node, equirectangular projection with
//! `x = R·(lon-lon0)·cos(lat0)`, `z = -R·(lat-lat0)`, R = 6 371 000 m.

use crate::dem::DemGrid;
use crate::osm::{OsmIndex, OsmWay};
use crate::types::{BridgeFeature, ImportStats, SurfaceKind};
use roads::{RoadCurve, RoadPoint};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

/// Everything an import run produced.
#[derive(Clone, Debug, Default)]
pub struct ImportResult {
    /// One curve per imported OSM way.
    pub curves: Vec<RoadCurve>,
    /// Bridge / viaduct features (also present as curves).
    pub bridges: Vec<BridgeFeature>,
    /// Aggregate counters.
    pub stats: ImportStats,
}

/// Highway values that become driveable road curves.
const KEEP_HIGHWAYS: &[&str] = &[
    "motorway",
    "trunk",
    "primary",
    "secondary",
    "tertiary",
    "unclassified",
    "residential",
    "service",
    "primary_link",
    "trunk_link",
    "motorway_link",
    "busway",
    "bus_guideway",
    "living_street",
];

/// Earth radius used by the local-meter projection (m).
const EARTH_R: f64 = 6_371_000.0;

/// Convert an OSM JSON document (`map.json` / Overpass / merged local
/// asset) into road curves.
///
/// * Ways whose `highway` is in [`KEEP_HIGHWAYS`] are imported
///   (footway/path/cycleway/steps/track/pedestrian/proposed/construction
///   are skipped by omission).
/// * `keep_unnamed = false` drops ways without a `name` tag; named ways
///   keep their tag as name, unnamed ones become `way/{id}`.
/// * When `dem` is given, every node is draped onto the terrain via
///   bilinear [`DemGrid::sample`].
/// * Cross-section defaults: lane width 3.5 m; two-way 1+1 lanes
///   (motorway/trunk/primary 2+2) or all-forward when `oneway=yes`;
///   `lanes` tag overrides the total (split round-robin, forward side
///   gets the extra lane); median 3.0 m when `lanes:psv` exists or the
///   way is a motorway.
/// * Surface: `surface` tag → [`SurfaceKind`] → `roads::surface` u8 on
///   every point; Mud/Compacted additionally set `RoadPoint.dirt`.
#[allow(clippy::too_many_lines)]
pub fn osm_doc_to_curves(
    doc: &Value,
    dem: Option<&DemGrid>,
    keep_unnamed: bool,
) -> ImportResult {
    let index = OsmIndex::new(doc);
    let (lat0, lon0) = index.first_node().unwrap_or((0.0, 0.0));
    let cos0 = lat0.to_radians().cos();

    let mut curves: Vec<RoadCurve> = Vec::new();
    let mut bridges: Vec<BridgeFeature> = Vec::new();
    let mut stats = ImportStats {
        ways: 0,
        curves: 0,
        bridges: 0,
        brt_ways: 0,
        elev_min: f32::MAX,
        elev_max: f32::MIN,
    };
    let mut elev_seen = false;

    for way in index.ways() {
        // Bridge extraction runs across ALL tagged ways (before the
        // highway filter) so bridges_from_doc and the import agree.
        if let Some(b) = bridge_feature(way, &index) {
            bridges.push(b);
        }

        let Some(hw) = way.tag("highway") else { continue };
        if !KEEP_HIGHWAYS.contains(&hw) {
            continue;
        }
        stats.ways += 1;
        let brt = is_brt(way);
        if brt {
            stats.brt_ways += 1;
        }

        let unnamed = way.tag("name").map(|n| n.trim().is_empty()).unwrap_or(true);
        if unnamed && !keep_unnamed {
            continue;
        }
        let display_name = if unnamed {
            format!("way/{}", way.id)
        } else {
            way.tag("name").unwrap_or("").trim().to_string()
        };

        // Node chain: dedupe consecutive repeats; on closed loops drop
        // the closing node (RoadCurve.closed re-adds the segment).
        let closed = way.is_closed();
        let mut lats: Vec<f64> = Vec::with_capacity(way.nodes.len());
        let mut lons: Vec<f64> = Vec::with_capacity(way.nodes.len());
        let mut prev: Option<i64> = None;
        for &nid in &way.nodes {
            if prev == Some(nid) {
                continue;
            }
            prev = Some(nid);
            if let Some([la, lo]) = index.node_pos(nid) {
                lats.push(la);
                lons.push(lo);
            }
        }
        if closed && lats.len() > 1 {
            lats.pop();
            lons.pop();
        }
        if lats.len() < if closed { 3 } else { 2 } {
            continue;
        }

        // Cross-section from tags.
        let oneway = matches!(
            way.tag("oneway"),
            Some("yes") | Some("true") | Some("1") | Some("-1")
        );
        let lanes_tag = way
            .tag("lanes")
            .and_then(|t| t.trim().parse::<usize>().ok())
            .filter(|&n| (1..=12).contains(&n));
        let major = matches!(hw, "motorway" | "trunk" | "primary");
        let (lanes, rev_lanes) = if oneway {
            (lanes_tag.unwrap_or(1), 0)
        } else if let Some(total) = lanes_tag {
            (total, (total / 2).min(total - 1))
        } else if major {
            (4, 2)
        } else {
            (2, 1)
        };
        let median = if way.tags.contains_key("lanes:psv") || hw == "motorway" {
            3.0
        } else {
            0.0
        };
        let surface = SurfaceKind::from_tag(way.tag("surface").unwrap_or(""));

        // Project to local meters (x east, z south).
        let n = lats.len();
        let mut coords: Vec<[f64; 2]> = Vec::with_capacity(n);
        for i in 0..n {
            let x = EARTH_R * (lons[i] - lon0).to_radians() * cos0;
            let z = -EARTH_R * (lats[i] - lat0).to_radians();
            coords.push([x, z]);
        }

        let mut pts: Vec<RoadPoint> = Vec::with_capacity(n);
        for i in 0..n {
            let elev = dem.and_then(|d| d.sample(lats[i], lons[i]));
            if let Some(e) = elev {
                stats.elev_min = stats.elev_min.min(e);
                stats.elev_max = stats.elev_max.max(e);
                elev_seen = true;
            }
            let mut p = RoadPoint::new(coords[i][0], coords[i][1]);
            p.name = display_name.clone();
            p.elev = elev.unwrap_or(0.0) as f64;
            p.bearing = bearing_at(i, &coords);
            p.prior_mag = 30.0;
            p.next_mag = 30.0;
            p.lanes = lanes;
            p.rev_lanes = rev_lanes;
            p.lane_width = 3.5;
            p.median = median;
            p.oneway = oneway;
            p.auto_bearing = true;
            apply_surface(&mut p, surface);
            pts.push(p);
        }

        let mut curve = RoadCurve::new(display_name);
        curve.points = pts;
        curve.closed = closed;
        curves.push(curve);
    }

    if !elev_seen {
        stats.elev_min = 0.0;
        stats.elev_max = 0.0;
    }
    stats.curves = curves.len();
    stats.bridges = bridges.len();
    ImportResult { curves, bridges, stats }
}

/// Surface bookkeeping on a RoadPoint, kept in ONE place:
/// `roads::surface` u8 constants on `RoadPoint.surface` plus the dirt
/// flag for the unpaved-earth family.
#[inline]
fn apply_surface(p: &mut RoadPoint, s: SurfaceKind) {
    p.surface = s.as_u8();
    p.dirt = s.is_dirt();
}

/// Bearing (atan2 over the xz plane) at point `i`: outgoing chord, or
/// the incoming chord at the chain end.
fn bearing_at(i: usize, coords: &[[f64; 2]]) -> f64 {
    let n = coords.len();
    if n < 2 {
        return 0.0;
    }
    let (a, b) = if i + 1 < n {
        (coords[i], coords[i + 1])
    } else {
        (coords[n - 2], coords[n - 1])
    };
    (b[1] - a[1]).atan2(b[0] - a[0])
}

/// True when the way carries BRT infrastructure: a `lanes:psv` tag or a
/// busway / bus_guideway highway class.
pub fn is_brt(way: &OsmWay) -> bool {
    way.tags.contains_key("lanes:psv")
        || matches!(way.tag("highway"), Some("busway") | Some("bus_guideway"))
}

/// Build a [`BridgeFeature`] from a way tagged `bridge` (any value but
/// `no`); `None` otherwise.
fn bridge_feature(way: &OsmWay, index: &OsmIndex) -> Option<BridgeFeature> {
    let bridge = way.tag("bridge")?;
    if bridge.eq_ignore_ascii_case("no") {
        return None;
    }
    Some(BridgeFeature {
        way_id: way.id,
        name: way.tag("name").unwrap_or("").to_string(),
        bridge: bridge.to_string(),
        layer: way
            .tag("layer")
            .and_then(|t| t.trim().parse::<i32>().ok())
            .unwrap_or(0),
        highway: way.tag("highway").unwrap_or("").to_string(),
        is_brt: is_brt(way),
        nodes: way.nodes.iter().filter_map(|id| index.node_pos(*id)).collect(),
        tags: way.tags.clone(),
    })
}

/// Extract all bridge / viaduct ways from an OSM document.
pub fn bridges_from_doc(doc: &Value) -> Vec<BridgeFeature> {
    let index = OsmIndex::new(doc);
    index.ways().iter().filter_map(|w| bridge_feature(w, &index)).collect()
}

/// Merge several OSM documents into one: concatenates `elements`,
/// de-duplicating by (type, id) keeping first occurrence.
pub fn merge_docs(docs: &[Value]) -> Value {
    let mut seen = std::collections::HashSet::new();
    let mut elements = Vec::new();
    for d in docs {
        let Some(arr) = d.get("elements").and_then(|v| v.as_array()) else {
            continue;
        };
        for e in arr {
            let key = (
                e.get("type").and_then(|v| v.as_str()).unwrap_or("").to_string(),
                e.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
            );
            if seen.insert(key) {
                elements.push(e.clone());
            }
        }
    }
    serde_json::json!({ "elements": elements })
}

/// Path of the workspace `assets/data` directory. Resolution order:
/// 1. `TRAFFIC_SIM_ASSETS` env var (points at the `assets` root — for
///    prebuilt binaries downloaded from GitHub Releases),
/// 2. `./assets/data` relative to the CWD (repo-checkout runs),
/// 3. this crate's compile-time manifest path (in-tree builds).
pub fn assets_data_dir() -> PathBuf {
    if let Ok(root) = std::env::var("TRAFFIC_SIM_ASSETS") {
        if !root.trim().is_empty() {
            return PathBuf::from(root).join("data");
        }
    }
    let cwd = PathBuf::from("assets/data");
    if cwd.is_dir() {
        return cwd;
    }
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets/data");
    p.canonicalize().unwrap_or(p)
}

/// The local z13 Terrarium tile covering both demo areas
/// (Kijazi AND Igesa/Sam Nujoma — 13/4988/4250).
fn local_kijazi_dem() -> io::Result<DemGrid> {
    let png = std::fs::read(assets_data_dir().join("mapbox/terrarium_13_4988_4250.png"))?;
    let mut dem = DemGrid::from_terrarium_png(&png).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("terrarium dem: {e}"))
    })?;
    dem.z = 13;
    dem.tx = 4988;
    dem.ty = 4250;
    Ok(dem)
}

/// OFFLINE Kijazi import: merges the local cached OSM extracts
/// `kijazi_area_roads.json` + `kijazi_flyover.json` +
/// `kijazi_bridge.json` (dedupe by type+id) and drapes the curves on the
/// local Terrarium DEM tile 13/4988/4250. Works without network.
pub fn kijazi_demo(cache_dir: &Path) -> io::Result<ImportResult> {
    let _ = cache_dir; // fully offline; parameter kept for API symmetry
    let dir = assets_data_dir();
    let area = crate::osm::load_local(&dir.join("kijazi_area_roads.json"))?;
    let flyover = crate::osm::load_local(&dir.join("kijazi_flyover.json"))?;
    let bridge = crate::osm::load_local(&dir.join("kijazi_bridge.json"))?;
    let doc = merge_docs(&[area, flyover, bridge]);
    let dem = local_kijazi_dem()?;
    Ok(osm_doc_to_curves(&doc, Some(&dem), true))
}

/// OFFLINE Igesa × Sam Nujoma import from the local
/// `igesa_samnujoma.json` extract (~19 MB, ~63k elements), draped on the
/// same local Terrarium DEM tile (13/4988/4250). The tile's mercator box
/// is verified against the Igesa bounds (lat -6.78…-6.76): points outside
/// the tile (or a missing/invalid DEM) simply keep `elev = 0.0` —
/// [`DemGrid::sample`] returns `None` there and the import degrades
/// gracefully to a flat scene.
pub fn igesa_demo(cache_dir: &Path) -> io::Result<ImportResult> {
    let _ = cache_dir;
    let dir = assets_data_dir();
    let doc = crate::osm::load_local(&dir.join("igesa_samnujoma.json"))?;
    let dem = match local_kijazi_dem() {
        Ok(d) => {
            let (west, south, east, north) = d.tile_box();
            let covered =
                south <= -6.78 && north >= -6.76 && west <= 39.214 && east >= 39.232;
            if covered {
                Some(d)
            } else {
                // tile does not span the Igesa area — import flat
                None
            }
        }
        Err(_) => None,
    };
    Ok(osm_doc_to_curves(&doc, dem.as_ref(), true))
}

/// The projection center used by [`osm_doc_to_curves`] (the document's
/// first node) so callers can align other coordinate systems (bridges,
/// terrain) with the imported curves.
pub fn projection_center(doc: &Value) -> (f64, f64) {
    OsmIndex::new(doc).first_node().unwrap_or((0.0, 0.0))
}
