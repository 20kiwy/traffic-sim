//! Flat cartography layers — the openstreetmap-carto ground stack.
//!
//! These are tessellated into an OPAQUE mesh (with depth) instead of the
//! debug immediate renderer: the main pass uses a read-only depth attachment
//! (depth comes from the prepass), so anything without prepass depth would
//! be covered by the sky. The carto mesh participates in the depth prepass
//! like every other map object.
//!
//! The procedural terrain chunks were removed: the ground is a single flat
//! quad covering the full OSM tile extent, so the whole tile is visible.
//!
//! Colors come from the real openstreetmap-carto palette (`maptools::carto`,
//! evaluated from the vendored `.mss` set — same source as the map-launcher
//! tile server), following the carto paint order: land → landuse →
//! landcover → water. The flat stack is drawn sunk by the camera-distance
//! depth quantum so it never ties with the road layer above it.
//!
//! Every paint here is bound to a layer-engine id (`maptools::layers`) —
//! the same catalog the carto preview tile server's Layers panel shows —
//! so the panel toggles the 3D ground stack with 100% parity.

use engine::Tesselator;
use geom::{vec2, vec3, LinearColor, Vec2, Vec3};
use maptools::layers::LayerVis;
use maptools::{carto, CartoData};
use std::collections::HashMap;

/// Fills a polygon on both sides — the map pipelines cull back faces and
/// OSM polygon ring order is arbitrary.
fn fill_poly(tess: &mut Tesselator, pts: &[Vec2], z: f32, col: LinearColor) {
    if pts.len() < 3 {
        return;
    }
    tess.set_color(col);
    tess.draw_filled_polygon(pts, z);
    let mut rev = pts.to_vec();
    rev.reverse();
    tess.draw_filled_polygon(&rev, z);
}

/// carto paint order: built-up landuse under the green landcover.
///
/// The per-polygon epsilon only separates overlapping SAME-KIND areas —
/// it must be counted per `@var` and capped: a flat `idx * 1e-4` over the
/// whole vec reached +0.36 m with 3646 areas, lifting late polygons ABOVE
/// the road layer (z 0.27) and erasing the roads (the "roads invisible"
/// bug).
fn area_z(var: &'static str, seq: &mut HashMap<&'static str, usize>) -> f32 {
    let base = if carto::CartoStyle::is_green_var(var) {
        0.045
    } else {
        0.03
    };
    let n = seq.entry(var).or_insert(0);
    let eps = (*n).min(40) as f32 * 1.0e-4;
    *n += 1;
    base + eps
}

/// Waterway stroke width per OSM class — the carto waterways layer's
/// z16-ish casing widths, in metres.
fn waterway_width(class: &str) -> f32 {
    match class {
        "river" => 8.0,
        "stream" => 3.0,
        _ => 2.0, // ditch / drain / canal-small
    }
}

pub fn draw_carto(carto_data: &CartoData, vis: &LayerVis, tess: &mut Tesselator, drop: f32) {
    let style = carto::global();
    let cat = maptools::layers::layer_catalog();

    // Full-extent flat ground (terrain chunk data removed) — @land-color.
    // `drop` sinks the flat stack below the depth-quantum tie with the road
    // layer (see State::carto_drop).
    if vis.on_in(&cat, "background") {
        if let Some(g) = carto_data.ground {
            fill_poly(
                tess,
                &[
                    vec2(g.ll.x, g.ll.y),
                    vec2(g.ur.x, g.ll.y),
                    vec2(g.ur.x, g.ur.y),
                    vec2(g.ll.x, g.ur.y),
                ],
                -drop,
                style.land(),
            );
        }
    }

    // landuse (residential/retail/industrial/…) + green landcover (forest,
    // grass, farmland, …) — the carto "Landuse fills" layer.
    if vis.on_in(&cat, "landuse") {
        let mut seq = HashMap::new();
        for (var, poly) in carto_data.areas.iter() {
            let z = area_z(var, &mut seq) - drop;
            fill_poly(tess, poly, z, style.linear(var));
        }
    }

    // Water areas — @water-color (carto "Water areas").
    if vis.on_in(&cat, "water-areas") {
        for w in &carto_data.water {
            fill_poly(tess, w, 0.06 - drop, style.water());
        }
    }

    // Linear waterways — @water-color strokes under the water areas
    // (carto "Waterways").
    if vis.on_in(&cat, "waterways") {
        tess.set_color(style.water());
        for (class, pts) in &carto_data.waterways {
            if pts.len() < 2 {
                continue;
            }
            tess.draw_polyline_full(
                pts.iter().map(|p| vec3(p.x, p.y, 0.05 - drop)),
                (pts[1] - pts[0]).try_normalize().unwrap_or(Vec2::X),
                (pts[pts.len() - 1] - pts[pts.len() - 2])
                    .try_normalize()
                    .unwrap_or(Vec2::X),
                waterway_width(class),
                0.0,
            );
        }
    }

    // Bridge railings — data-driven from the traffic-sim BridgeSpec
    // (deck width, railing height). The elevated deck itself is the road
    // mesh at deck z (carto @bridge-casing black outline comes from the
    // road renderer); the piers are drawn automatically by the road
    // renderer wherever the deck is above ground.
    if vis.on_in(&cat, "bridges") {
        for b in &carto_data.bridges {
            let half = (b.spec.deck_width_m as f32) * 0.5;
            let rail_h = b.spec.rail_h_m as f32;
            if b.pts.len() < 2 {
                continue;
            }
            // railing paint = the road's carto casing, darkened like the carto
            // bridge furniture
            let col = style.building_line();
            tess.set_color(col);
            for side in [-1.0f32, 1.0] {
                for pair in b.pts.windows(2) {
                    let (a, bpt) = (pair[0], pair[1]);
                    let dir = (bpt.xy() - a.xy()).try_normalize().unwrap_or(Vec2::X);
                    let perp = dir.perpendicular() * (half * side);
                    let from = vec3(a.x + perp.x, a.y + perp.y, a.z + rail_h);
                    let to = vec3(bpt.x + perp.x, bpt.y + perp.y, bpt.z + rail_h);
                    tess.draw_stroke(from, to, 0.22);
                }
            }
        }
    }
}
