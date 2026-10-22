//! Flat cartography layers — ground, water, parks and bridge railings.
//!
//! These are tessellated into an OPAQUE mesh (with depth) instead of the
//! debug immediate renderer: the main pass uses a read-only depth attachment
//! (depth comes from the prepass), so anything without prepass depth would
//! be covered by the sky. The carto mesh participates in the depth prepass
//! like every other map object.
//!
//! The procedural terrain chunks were removed: the ground is a single flat
//! quad covering the full OSM tile extent, so the whole tile is visible.

use engine::Tesselator;
use geom::{vec2, vec3, LinearColor, Vec2, Vec3};
use maptools::CartoData;

fn ground_col() -> LinearColor {
    // desaturated warm green — carto land
    LinearColor {
        r: 0.55,
        g: 0.62,
        b: 0.47,
        a: 1.0,
    }
}

fn park_col() -> LinearColor {
    LinearColor {
        r: 0.42,
        g: 0.58,
        b: 0.38,
        a: 1.0,
    }
}

fn water_col() -> LinearColor {
    LinearColor {
        r: 0.35,
        g: 0.55,
        b: 0.65,
        a: 1.0,
    }
}

fn rail_col() -> LinearColor {
    LinearColor {
        r: 0.78,
        g: 0.78,
        b: 0.76,
        a: 1.0,
    }
}

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

pub fn draw_carto(carto: &CartoData, tess: &mut Tesselator) {
    // Full-extent flat ground (terrain chunk data removed).
    if let Some(g) = carto.ground {
        fill_poly(
            tess,
            &[
                vec2(g.ll.x, g.ll.y),
                vec2(g.ur.x, g.ll.y),
                vec2(g.ur.x, g.ur.y),
                vec2(g.ll.x, g.ur.y),
            ],
            0.0,
            ground_col(),
        );
    }

    for p in &carto.parks {
        fill_poly(tess, p, 0.04, park_col());
    }
    for w in &carto.water {
        fill_poly(tess, w, 0.06, water_col());
    }

    // Bridge railings — data-driven from the traffic-sim BridgeSpec
    // (deck width, railing height). The elevated deck itself is the road
    // mesh at deck z; the piers are drawn automatically by the road
    // renderer wherever the deck is above ground.
    for b in &carto.bridges {
        let half = (b.spec.deck_width_m as f32) * 0.5;
        let rail_h = b.spec.rail_h_m as f32;
        if b.pts.len() < 2 {
            continue;
        }
        tess.set_color(rail_col());
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
