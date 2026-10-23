//! Overpass XML extracts — supplementary cartography layers
//! (buildings, water, landcover/landuse) parsed from the same `.osm` files
//! the traffic-sim project stores next to the compact ways JSON.
//!
//! Every polygon is tagged with the openstreetmap-carto `@var` that paints
//! it (resolved through [`carto::CartoStyle::area_var`]) so the renderer
//! stays 100% data-driven — same palette as the map-launcher tile server.

use common::FastMap;
use geom::{vec2, OBB, Polygon, Vec2};
use prototypes::BuildingGen;
use simulation::map::{BuildingKind, Map};

use crate::{carto, poly_obb, CartoData};

pub fn load(xml: &str, map: &mut Map, carto_data: &mut CartoData) {
    let doc = match roxmltree::Document::parse(xml) {
        Ok(d) => d,
        Err(e) => {
            log::warn!("maptools: failed to parse Overpass XML: {}", e);
            return;
        }
    };

    // ---- nodes ----
    let mut nodes: FastMap<u64, (f64, f64)> = FastMap::default();
    for n in doc.descendants().filter(|t| t.has_tag_name("node")) {
        let (Some(id), Some(lat), Some(lon)) = (
            n.attribute("id").and_then(|v| v.parse::<u64>().ok()),
            n.attribute("lat").and_then(|v| v.parse::<f64>().ok()),
            n.attribute("lon").and_then(|v| v.parse::<f64>().ok()),
        ) else {
            continue;
        };
        nodes.insert(id, (lon, lat));
    }

    // ---- ways ----
    for w in doc.descendants().filter(|t| t.has_tag_name("way")) {
        let mut refs: Vec<u64> = Vec::new();
        let mut tags: Vec<(&str, &str)> = Vec::new();
        for child in w.descendants() {
            if child.has_tag_name("nd") {
                if let Some(r) = child.attribute("ref").and_then(|v| v.parse::<u64>().ok()) {
                    refs.push(r);
                }
            } else if child.has_tag_name("tag") {
                if let (Some(k), Some(v)) = (child.attribute("k"), child.attribute("v")) {
                    tags.push((k, v));
                }
            }
        }
        if refs.is_empty() {
            continue;
        }
        let tag = |k: &str| tags.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);

        let pts: Vec<Vec2> = refs
            .iter()
            .filter_map(|r| nodes.get(r))
            .map(|&(lo, la)| {
                let (x, y) = crate::ll2m(lo, la);
                vec2(x as f32, y as f32)
            })
            .collect();
        if pts.is_empty() {
            continue;
        }

        // linear waterways (river/canal/stream/…) — the carto waterways
        // layer paints them as @water-color strokes; riverbanks/polygon
        // water fall through to the area branch below.
        let waterway = tag("waterway").unwrap_or("");
        if let Some(class) = waterway_line_class(waterway) {
            if pts.len() >= 2 && tag("area").is_none() && tag("natural").is_none() {
                let mut dedup = vec![pts[0]];
                for p in &pts[1..] {
                    if (*p - dedup[dedup.len() - 1]).mag2() > 1.0 {
                        dedup.push(*p);
                    }
                }
                if dedup.len() >= 2 {
                    carto_data.grow_bounds(&dedup);
                    carto_data.waterways.push((class, dedup));
                }
                continue;
            }
        }

        if refs.len() < 3 {
            continue;
        }
        if pts.len() < 3 {
            continue;
        }

        // buildings (carto @building-fill — colors come from colors.lua)
        if tag("building").is_some() && tag("building").unwrap() != "no" {
            if let Some(obb) = poly_obb(&pts) {
                if map
                    .build_special_building(&obb, BuildingKind::House, BuildingGen::House, None, None)
                    .is_some()
                {
                    carto_data.buildings += 1;
                }
                carto_data.grow_bounds(&pts);
            }
            // Built-up ground: the extract carries no landuse=residential
            // polygons, so the residential @var halo is derived from the
            // real building footprints (expanded ~1.8x) — the same visual
            // job landuse polygons do on the openstreetmap-carto map at
            // z12-13, and it makes the white road casings read.
            let centroid = pts.iter().fold(Vec2::ZERO, |a, p| a + *p) / pts.len() as f32;
            let halo: Vec<Vec2> = pts
                .iter()
                .map(|p| centroid + (*p - centroid) * 1.8)
                .collect();
            if Polygon(halo.clone()).area() > 30.0 {
                carto_data.areas.push(("residential", halo));
            }
            continue;
        }

        // water
        let natural = tag("natural").unwrap_or("");
        let waterway = tag("waterway").unwrap_or("");
        let water = tag("water").unwrap_or("");
        if matches!(natural, "water" | "riverbank" | "bay")
            || waterway == "riverbank"
            || (waterway == "river" && tag("area").is_some())
            || !water.is_empty()
        {
            if Polygon(pts.clone()).area() > 30.0 {
                carto_data.grow_bounds(&pts);
                carto_data.water.push(pts);
            }
            continue;
        }

        // landcover / landuse — tagged with the carto @var that paints it
        if let Some(var) = landcover_var(&tag) {
            if Polygon(pts.clone()).area() > 30.0 {
                carto_data.grow_bounds(&pts);
                carto_data.areas.push((var, pts));
            }
        }
    }
}

/// `waterway=*` linear class → the carto waterways layer's paintable
/// classes (landcover.mss / water.mss: river, canal, stream, ditch, drain).
fn waterway_line_class(v: &str) -> Option<&'static str> {
    match v {
        "river" | "canal" => Some("river"),
        "stream" => Some("stream"),
        "ditch" | "drain" => Some("ditch"),
        _ => None,
    }
}

/// OSM tag set → openstreetmap-carto `@var`. Mirrors the map-launcher
/// landcover.mss classification order (landuse → natural → leisure/amenity).
fn landcover_var<'a>(tag: &impl Fn(&str) -> Option<&'a str>) -> Option<&'static str> {
    let landuse = tag("landuse").unwrap_or("");
    if let Some(var) = carto::CartoStyle::area_var(landuse) {
        if landuse != "no" {
            return Some(var);
        }
    }
    let natural = tag("natural").unwrap_or("");
    if !natural.is_empty() {
        if let Some(var) = carto::CartoStyle::area_var(natural) {
            return Some(var);
        }
    }
    let leisure = tag("leisure").unwrap_or("");
    if !leisure.is_empty() {
        if let Some(var) = carto::CartoStyle::area_var(leisure) {
            return Some(var);
        }
    }
    let amenity = tag("amenity").unwrap_or("");
    if !amenity.is_empty() {
        if let Some(var) = carto::CartoStyle::area_var(amenity) {
            return Some(var);
        }
    }
    None
}

/// Re-exported for tests: minimal-area-ish OBB from a polygon footprint.
pub fn _obb(pts: &[Vec2]) -> Option<OBB> {
    poly_obb(pts)
}
