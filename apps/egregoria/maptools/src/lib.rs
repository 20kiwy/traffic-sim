//! maptools — OSM → Egregoria cartography import tools.
//!
//! Everything about loading OSM data into an Egregoria [`Map`] lives in this
//! separate library crate — none of it is hardcoded in the app:
//!
//! * [`schema`] — the compact OSM ways JSON format used by the egui app
//!   (`traffic-sim/assets/dsm/*.json`, fields `pts/hw/n/l/b/ln/ow`).
//! * [`overpass`] — Overpass XML extracts for the cartography layers
//!   (buildings, water, parks).
//! * Road classification mirrors the traffic-sim properties model
//!   (lanes per direction, widths, speed classes, surfaces).
//! * Bridges use the traffic-sim `bridge` crate engineering specs
//!   (deck width from lanes, deck thickness, railing height, clearance)
//!   so flyovers render elevated with real data.
//! * Projection matches the egui app exactly (`GEO_ANCHOR`, equirectangular).

pub mod carto;
pub mod overpass;
pub mod schema;

use std::collections::HashSet;
use std::time::Instant;

use common::FastMap;
use flat_spatial::Grid;
use geom::{vec2, vec3, AABB, OBB, PolyLine3, Vec2, Vec3};
use prototypes::BuildingGen;
use simulation::map::{
    BuildingKind, IntersectionID, LanePattern, LanePatternBuilder, Map, RoadID, RoadSegmentKind,
    ROAD_Z_OFFSET,
};

/// Same anchor as the egui app (`roadkit::dsm::GEO_ANCHOR`):
/// Kijazi interchange center (Morogoro × Sam Nujoma).
pub const GEO_ANCHOR: (f64, f64) = (39.2220, -6.7720);
const M_PER_DEG_LON: f64 = 110_573.0; // 111 320 * cos(6.81°)
const M_PER_DEG_LAT: f64 = 110_574.0;

/// lon/lat → scene metres, identical to the egui app's `ll2m`.
pub fn ll2m(lon: f64, lat: f64) -> (f64, f64) {
    (
        (lon - GEO_ANCHOR.0) * M_PER_DEG_LON,
        -(lat - GEO_ANCHOR.1) * M_PER_DEG_LAT,
    )
}

/// Default dataset — the exact same files the egui app embeds.
pub static KIJAZI_WAYS: &str = include_str!("../../../../assets/dsm/kijazi.json");
pub static KIJAZI_XML: &str = include_str!("../../../../assets/dsm/kijazi.osm");

// ---------------------------------------------------------------------------
// Result data handed to the renderer / UI
// ---------------------------------------------------------------------------

/// Full road properties carried over from the traffic-sim model.
#[derive(Clone, Debug)]
pub struct RoadProps {
    pub name: String,
    pub highway: String,
    pub lanes_fwd: usize,
    pub lanes_bwd: usize,
    pub oneway: bool,
    /// `roads::surface` constant from the traffic-sim project.
    pub surface: u8,
    pub maxspeed_kmh: f32,
    pub width_m: f32,
    pub layer: i8,
    pub bridge: Option<bridge::BridgeSpec>,
}

#[derive(Clone, Debug)]
pub struct BridgeInfo {
    pub road: RoadID,
    pub name: String,
    /// Traffic-sim bridge engineering spec (deck width, thickness, rail
    /// height, clearance, pier spacing, kind).
    pub spec: bridge::BridgeSpec,
    /// Elevated deck centerline, src → dst.
    pub pts: Vec<Vec3>,
}

/// Cartography data produced by the import and consumed by the app
/// (ground extent, flat landcover/water polygons, bridge + road metadata).
#[derive(Default)]
pub struct CartoData {
    pub ground: Option<AABB>,
    /// Landcover / landuse polygons tagged with the openstreetmap-carto
    /// `@var` that paints them (resolved by [`carto::CartoStyle::area_var`]).
    pub areas: Vec<(&'static str, Vec<Vec2>)>,
    pub water: Vec<Vec<Vec2>>,
    pub buildings: usize,
    pub roads: usize,
    pub bridges: Vec<BridgeInfo>,
    pub props: FastMap<RoadID, RoadProps>,
}

impl CartoData {
    pub fn grow_bounds(&mut self, pts: &[Vec2]) {
        for p in pts {
            self.grow_bounds3(&[vec3(p.x, p.y, 0.0)]);
        }
    }

    pub fn grow_bounds3(&mut self, pts: &[Vec3]) {
        for p in pts {
            let aabb = match self.ground {
                Some(aabb) => aabb,
                None => {
                    self.ground = Some(AABB {
                        ll: vec2(p.x, p.y),
                        ur: vec2(p.x, p.y),
                    });
                    continue;
                }
            };
            self.ground = Some(AABB {
                ll: vec2(aabb.ll.x.min(p.x), aabb.ll.y.min(p.y)),
                ur: vec2(aabb.ur.x.max(p.x), aabb.ur.y.max(p.y)),
            });
        }
    }

    fn compute_ground(&self) -> Option<AABB> {
        self.ground.map(|aabb| {
            let pad = 80.0;
            AABB {
                ll: vec2(aabb.ll.x - pad, aabb.ll.y - pad),
                ur: vec2(aabb.ur.x + pad, aabb.ur.y + pad),
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Classification — mirrors the egui app + traffic-sim lane model
// ---------------------------------------------------------------------------

pub struct Class {
    lanes: usize,
    walking: bool,
    sidewalks: bool,
    speed_kmh: f32,
    surface: u8,
}

fn classify(hw: &str) -> Option<Class> {
    let c = |lanes: usize, speed: f32, sidewalks: bool| {
        Some(Class {
            lanes,
            walking: false,
            sidewalks,
            speed_kmh: speed,
            surface: roads::surface::ASPHALT,
        })
    };
    match hw {
        "motorway" => c(3, 110.0, false),
        "motorway_link" => c(1, 60.0, false),
        "trunk" => c(2, 90.0, true),
        "trunk_link" => c(1, 60.0, false),
        "primary" => c(2, 70.0, true),
        "primary_link" => c(1, 50.0, false),
        "secondary" => c(1, 60.0, true),
        "secondary_link" => c(1, 40.0, false),
        "tertiary" => c(1, 50.0, true),
        "tertiary_link" => c(1, 40.0, false),
        "residential" | "unclassified" | "living_street" => c(1, 40.0, true),
        "service" => c(1, 20.0, false),
        "footway" | "path" | "pedestrian" | "steps" | "cycleway" => Some(Class {
            lanes: 0,
            walking: true,
            sidewalks: true,
            speed_kmh: 15.0,
            surface: roads::surface::CONCRETE,
        }),
        _ => None,
    }
}

/// Traffic-sim lane model → Egregoria `LanePattern`.
fn lane_pattern(cl: &Class, lanes_total: Option<usize>, oneway: bool) -> (LanePattern, usize, usize) {
    if cl.walking {
        // n_lanes(0) → no driving lanes, sidewalks on both sides.
        let p = LanePatternBuilder::new()
            .n_lanes(0)
            .sidewalks(true)
            .parking(false)
            .build();
        return (p, 0, 0);
    }
    let per_dir = match lanes_total {
        Some(t) => {
            if oneway {
                t.max(1)
            } else {
                (((t as f32) / 2.0).ceil() as usize).max(1)
            }
        }
        None => cl.lanes,
    };
    let n = per_dir.clamp(1, 3);
    let bwd = if oneway { 0 } else { n };
    let p = LanePatternBuilder::new()
        .n_lanes(n as u32)
        .sidewalks(cl.sidewalks)
        .parking(true) // parking lanes generate the spots random traffic needs
        .one_way(oneway)
        .speed_limit(cl.speed_kmh / 3.6)
        .build();
    (p, n, bwd)
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

/// Oriented bounding box from a building footprint (longest edge = axis).
pub fn poly_obb(pts: &[Vec2]) -> Option<OBB> {
    if pts.len() < 3 {
        return None;
    }
    let (mut best_len2, mut axis) = (0.0f32, Vec2::X);
    for i in 0..pts.len() {
        let d = pts[(i + 1) % pts.len()] - pts[i];
        let l2 = d.mag2();
        if l2 > best_len2 {
            best_len2 = l2;
            axis = d;
        }
    }
    let axis = axis.try_normalize().unwrap_or(Vec2::X);
    let perp = axis.perpendicular();

    let centroid = pts.iter().fold(Vec2::ZERO, |acc, p| acc + *p) / pts.len() as f32;

    let (mut minu, mut maxu, mut minv, mut maxv) =
        (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in pts {
        let d = *p - centroid;
        let (u, v) = (d.dot(axis), d.dot(perp));
        minu = minu.min(u);
        maxu = maxu.max(u);
        minv = minv.min(v);
        maxv = maxv.max(v);
    }
    let center = centroid + axis * ((minu + maxu) * 0.5) + perp * ((minv + maxv) * 0.5);
    let w = (maxu - minu).max(1.0);
    let h = (maxv - minv).max(1.0);
    if w * h > 20_000.0 {
        return None; // skip huge footprints (terminals, depots)
    }
    Some(OBB::new(center, axis, w, h))
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Returns `(intersection, exact position)` — snapped intersections keep
/// their creation position, and road endpoints must be pulled to it (map
/// invariant: first/last road point == intersection pos + ROAD_Z_OFFSET).
fn snap_or_add(
    map: &mut Map,
    grid: &mut Grid<IntersectionID, Vec2>,
    p: Vec2,
    z: f32,
    idx: &mut FastMap<IntersectionID, u32>,
    next: &mut u32,
) -> Option<(IntersectionID, Vec2)> {
    if let Some((h, _)) = grid.query_around(p, 10.0).next() {
        let (pos, id) = grid.get(h)?;
        return Some((*id, pos));
    }
    // Intersections live ROAD_Z_OFFSET below the road surface (map
    // invariant: road point z == inter pos z + ROAD_Z_OFFSET).
    let id = map.osm_add_intersection(vec3(p.x, p.y, z - ROAD_Z_OFFSET));
    grid.insert(p, id);
    idx.insert(id, *next);
    *next += 1;
    Some((id, p))
}

/// Builds the whole OSM map into `map` and returns the cartography data.
///
/// * `ways_json` — compact ways JSON (same files as the egui app).
/// * `overpass_xml` — optional Overpass XML for buildings/water/parks.
pub fn load_osm_map(map: &mut Map, ways_json: &str, overpass_xml: Option<&str>) -> CartoData {
    let t = Instant::now();
    let mut carto = CartoData::default();

    let ways: Vec<schema::OsmWay> = match serde_json::from_str::<schema::OsmWays>(ways_json) {
        Ok(v) => v.ways,
        Err(e) => {
            log::error!("maptools: bad ways JSON: {}", e);
            Vec::new()
        }
    };

    // Endpoint snapping — separate levels for ground and bridge roads so a
    // flyover never merges with the road it crosses.
    let mut grid_ground: Grid<IntersectionID, Vec2> = Grid::new(50);
    let mut grid_bridge: Grid<IntersectionID, Vec2> = Grid::new(50);
    let mut inter_idx: FastMap<IntersectionID, u32> = FastMap::default();
    let mut next_idx = 0u32;
    let mut seen_pairs: HashSet<(u32, u32)> = HashSet::new();

    for w in &ways {
        let Some(cl) = classify(&w.hw) else { continue };

        let mut pts: Vec<Vec3> = w
            .pts
            .iter()
            .map(|&(lo, la)| {
                let (x, y) = ll2m(lo, la);
                vec3(x as f32, y as f32, ROAD_Z_OFFSET)
            })
            .collect();
        if pts.len() < 2 {
            continue;
        }

        // length gate — drop mapping slivers, keep footpaths
        let mut len = 0.0f32;
        for i in 1..pts.len() {
            len += (pts[i].xy() - pts[i - 1].xy()).mag();
        }
        if len < if cl.walking { 8.0 } else { 18.0 } {
            continue;
        }

        // bridge spec from the traffic-sim bridge engineering crate
        let lanes_total = w
            .lanes_total()
            .unwrap_or(cl.lanes * if w.is_oneway() { 1 } else { 2 });
        let spec = if w.is_bridge() {
            let kind_tag = if w.b >= 2 || w.l >= 2 { "viaduct" } else { "yes" };
            let mut s = bridge::BridgeSpec::from_tags(&w.n, kind_tag, w.l as i32, lanes_total, false);
            s.clearance_m = if w.l >= 2 { 6.2 } else { 4.8 };
            Some(s)
        } else {
            None
        };
        if let Some(ref s) = spec {
            let deck_z = (s.clearance_m + s.deck_th_m) as f32;
            for p in &mut pts {
                p.z = ROAD_Z_OFFSET + deck_z;
            }
        }

        let (pattern, fwd, bwd) = lane_pattern(&cl, w.lanes_total(), w.is_oneway());

        // endpoints: snap within the same level (ground vs deck)
        let is_bridge_way = spec.is_some();
        let src = {
            let grid = if is_bridge_way { &mut grid_bridge } else { &mut grid_ground };
            snap_or_add(map, grid, pts[0].xy(), pts[0].z, &mut inter_idx, &mut next_idx)
        };
        let dst = {
            let grid = if is_bridge_way { &mut grid_bridge } else { &mut grid_ground };
            snap_or_add(map, grid, pts.last().unwrap().xy(), pts.last().unwrap().z, &mut inter_idx, &mut next_idx)
        };
        let (Some((src_id, src_xy)), Some((dst_id, dst_xy))) = (src, dst) else {
            continue;
        };
        if src_id == dst_id {
            continue;
        }
        // Pull the road endpoints exactly onto the intersection positions.
        pts[0].x = src_xy.x;
        pts[0].y = src_xy.y;
        let last = pts.len() - 1;
        pts[last].x = dst_xy.x;
        pts[last].y = dst_xy.y;
        let (ia, ib) = (inter_idx[&src_id], inter_idx[&dst_id]);
        if !seen_pairs.insert((ia.min(ib), ia.max(ib))) {
            continue; // duplicate segment between the same intersections
        }

        let segment = RoadSegmentKind::Arbitrary(PolyLine3::new(pts.clone()));
        let Some(rid) = map.osm_connect(src_id, dst_id, &pattern, segment) else {
            continue;
        };

        carto.roads += 1;
        carto.props.insert(
            rid,
            RoadProps {
                name: w.n.clone(),
                highway: w.hw.clone(),
                lanes_fwd: fwd,
                lanes_bwd: bwd,
                oneway: w.is_oneway(),
                surface: cl.surface,
                maxspeed_kmh: cl.speed_kmh,
                width_m: pattern.width(),
                layer: w.l,
                bridge: spec.clone(),
            },
        );
        if let Some(s) = spec {
            carto.bridges.push(BridgeInfo {
                road: rid,
                name: if w.n.is_empty() {
                    "Bridge".to_string()
                } else {
                    w.n.clone()
                },
                spec: s,
                pts: pts.clone(),
            });
        }
        carto.grow_bounds3(&pts);
    }

    // Remove orphan intersections BEFORE the building phase —
    // build_special_building checks map invariants.
    map.osm_finish();

    if let Some(xml) = overpass_xml {
        overpass::load(xml, map, &mut carto);
    }

    carto.ground = carto.compute_ground();
    map.osm_finish();

    log::info!(
        "maptools: OSM import done in {:.1}s — {} roads, {} bridges, {} buildings, {} water, {} landcover areas",
        t.elapsed().as_secs_f32(),
        carto.roads,
        carto.bridges.len(),
        carto.buildings,
        carto.water.len(),
        carto.areas.len()
    );

    carto
}
