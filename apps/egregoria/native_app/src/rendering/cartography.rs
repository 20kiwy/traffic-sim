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

use engine::Tesselator;
use geom::{vec2, vec3, LinearColor, Vec2, Vec3};
use maptools::{carto, CartoData};

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
fn area_z(var: &str, idx: usize) -> f32 {
    let base = if carto::CartoStyle::is_green_var(var) {
        0.045
    } else {
        0.03
    };
    // tiny per-polygon epsilon so overlapping same-kind areas never z-fight
    base + (idx as f32) * 1.0e-4
}

pub fn draw_carto(carto_data: &CartoData, tess: &mut Tesselator, drop: f32) {
    let style = carto::global();

    // Full-extent flat ground (terrain chunk data removed) — @land-color.
    // `drop` sinks the flat stack below the depth-quantum tie with the road
    // layer (see State::carto_drop).
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

    // landuse (residential/retail/industrial/…) then landcover (forest,
    // grass, farmland, …) — exactly the carto stack order.
    for (i, (var, poly)) in carto_data.areas.iter().enumerate() {
        fill_poly(tess, poly, area_z(var, i) - drop, style.linear(var));
    }

    for w in &carto_data.water {
        fill_poly(tess, w, 0.06 - drop, style.water());
    }

    // Bridge railings — data-driven from the traffic-sim BridgeSpec
    // (deck width, railing height). The elevated deck itself is the road
    // mesh at deck z (carto @bridge-casing black outline comes from the
    // road renderer); the piers are drawn automatically by the road
    // renderer wherever the deck is above ground.
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
