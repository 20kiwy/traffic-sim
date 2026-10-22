//! Overpass XML extracts — supplementary cartography layers
//! (buildings, water, parks) parsed from the same `.osm` files the
//! traffic-sim project stores next to the compact ways JSON.

use common::FastMap;
use geom::{vec2, OBB, Polygon, Vec2};
use prototypes::BuildingGen;
use simulation::map::{BuildingKind, Map};

use crate::{poly_obb, CartoData};

pub fn load(xml: &str, map: &mut Map, carto: &mut CartoData) {
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
        if refs.len() < 3 {
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
        if pts.len() < 3 {
            continue;
        }

        // buildings
        if tag("building").is_some() && tag("building").unwrap() != "no" {
            if let Some(obb) = poly_obb(&pts) {
                if map
                    .build_special_building(&obb, BuildingKind::House, BuildingGen::House, None, None)
                    .is_some()
                {
                    carto.buildings += 1;
                }
                carto.grow_bounds(&pts);
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
                carto.grow_bounds(&pts);
                carto.water.push(pts);
            }
            continue;
        }

        // parks / green areas
        let leisure = tag("leisure").unwrap_or("");
        if matches!(leisure, "park" | "garden" | "pitch" | "playground" | "golf_course")
            || matches!(natural, "wood" | "grass" | "scrub" | "forest" | "wetland")
        {
            if Polygon(pts.clone()).area() > 30.0 {
                carto.grow_bounds(&pts);
                carto.parks.push(pts);
            }
        }
    }
}

/// Re-exported for tests: minimal-area-ish OBB from a polygon footprint.
pub fn _obb(pts: &[Vec2]) -> Option<OBB> {
    poly_obb(pts)
}
