//! 3D scene assembly + rendering.
//!
//! Mirrors the godot-road-generator preview: procedural road meshes
//! (roads::mesh3d) with GLB prop assets joined along the roads
//! (vehicle / milestone / tree / streetlight / junctionMarker).
//! Scene space = METERS, x east, y up, z south.

use crate::library::AssetLibrary;
use crate::raster::Frame;
use crate::textures::TextureLibrary;
use gdmath::vec3::Vec3;
use gdmath::Mat4;
use roads::junction::{marking_parts_for, mesh_parts_for};
use roads::mesh3d::{build_parts, MeshQuality};
use roads::model::RoadCurve;

#[derive(Clone)]
pub struct ScenePart {
    pub positions: Vec<Vec3>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
    /// per-vertex uvs (when textured)
    pub uv: Vec<[f32; 2]>,
    /// texture slot (roadkit::textures); -1 = flat color
    pub tex: i32,
}

#[derive(Clone)]
pub struct PropDraw {
    pub asset: usize,
    pub model: Mat4,
    /// texture slot override for UV-carrying props (junction prefabs sample
    /// the GRG trim sheet); -1 = plain vertex colors
    pub tex: i32,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SceneStats {
    pub curves: usize,
    pub points: usize,
    pub props: usize,
    pub tris: usize,
    pub length_m: f64,
}

#[derive(Clone)]
pub struct Scene {
    pub parts: Vec<ScenePart>,
    pub props: Vec<PropDraw>,
    /// ground plane + grid
    pub ground: Vec<ScenePart>,
    pub radius_m: f32,
    pub stats: SceneStats,
}

/// Assemble the render scene from curves + junctions + prop instances.
/// `center` is subtracted from all positions (keeps f32 precision).
/// Wrapper kept for CLI / MCP / selftest callers: no lane fins, no terrain.
pub fn build_scene(
    curves: &[RoadCurve],
    junctions: &[roads::junction::Junction],
    props: &[crate::props::PropInstance],
    library: &AssetLibrary,
    show_grid: bool,
) -> Scene {
    build_scene_full(curves, junctions, props, &[], library, show_grid, false, None)
}

/// Full scene assembly: roads + junctions + props + AI-lane fins
/// (GRG `draw_lanes` preview) + the terrain heightfield
/// (RoadTerrain3DConnector). `center` is subtracted from all positions
/// (keeps f32 precision). Reachable as `roadkit::scene::build_scene_full`
/// (the `scene` module is `pub` — no lib.rs re-export needed).
#[allow(clippy::too_many_arguments)]
pub fn build_scene_full(
    curves: &[RoadCurve],
    junctions: &[roads::junction::Junction],
    props: &[crate::props::PropInstance],
    lanes: &[roads::lane::RoadLane],
    library: &AssetLibrary,
    show_grid: bool,
    show_lanes: bool,
    terrain: Option<&roads::terrain::TerrainState>,
) -> Scene {
    let _ = library; // props carry their own resolved indices
    // bbox of all curve points
    let (mut min_x, mut min_z) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let mut total_points = 0usize;
    let mut total_len = 0.0f64;
    for c in curves {
        total_points += c.points.len();
        total_len += c.length_m();
        for p in &c.points {
            min_x = min_x.min(p.x);
            min_z = min_z.min(p.z);
            max_x = max_x.max(p.x);
            max_z = max_z.max(p.z);
        }
        for p in &c.centerline(6.0) {
            min_x = min_x.min(p.x as f64);
            min_z = min_z.min(p.z as f64);
            max_x = max_x.max(p.x as f64);
            max_z = max_z.max(p.z as f64);
        }
    }
    if !min_x.is_finite() {
        (min_x, min_z, max_x, max_z) = (-50.0, -50.0, 50.0, 50.0);
    }
    let cx = ((min_x + max_x) / 2.0) as f32;
    let cz = ((min_z + max_z) / 2.0) as f32;

    // road meshes (textured)
    let mut parts: Vec<ScenePart> = Vec::new();
    let mut tris = 0usize;
    for c in curves {
        for part in build_parts(c, MeshQuality { markings: true, sample_m: 4.0 }) {
            tris += if part.indices.is_empty() {
                part.positions.len() / 3
            } else {
                part.indices.len() / 3
            };
            parts.push(ScenePart {
                positions: part
                    .positions
                    .iter()
                    .map(|v| Vec3::new(v.x - cx, v.y, v.z - cz))
                    .collect(),
                indices: part.indices,
                color: part.color,
                uv: part.uv,
                tex: part.tex,
            });
        }
    }

    // junction patches (crossings, roundabouts) — only when no GLB prefab
    // covers them (the prefab IS the intersection surface then; painted
    // markings still go on top of the prefab tiles)
    for j in junctions {
        if j.prefab.is_some() {
            for part in marking_parts_for(j, curves) {
                parts.push(ScenePart {
                    positions: part
                        .positions
                        .iter()
                        .map(|v| Vec3::new(v.x - cx, v.y, v.z - cz))
                        .collect(),
                    indices: part.indices,
                    color: part.color,
                    uv: part.uv,
                    tex: part.tex,
                });
            }
            continue;
        }
        for part in mesh_parts_for(j, curves) {
            tris += if part.indices.is_empty() {
                part.positions.len() / 3
            } else {
                part.indices.len() / 3
            };
            parts.push(ScenePart {
                positions: part
                    .positions
                    .iter()
                    .map(|v| Vec3::new(v.x - cx, v.y, v.z - cz))
                    .collect(),
                indices: part.indices,
                color: part.color,
                uv: part.uv,
                tex: part.tex,
            });
        }
    }

    // props
    let mut prop_draws: Vec<PropDraw> = Vec::new();
    for p in props {
        let Some(idx) = library.index.get(&p.asset).copied() else {
            continue;
        };
        prop_draws.push(PropDraw {
            asset: idx,
            model: Mat4::prop_model(
                Vec3::new(p.pos.x - cx, p.pos.y, p.pos.z - cz),
                p.yaw as f32,
                p.scale as f32,
            ),
            tex: -1,
        });
    }

    // junction GLB prefabs (WFC-collapsed tiles). Their lane markings are
    // geometry UV-mapped to the GRG trim sheet, so they render textured.
    let mut island_discs: Vec<ScenePart> = Vec::new();
    for j in junctions {
        let Some(pf) = &j.prefab else { continue };
        let Some(idx) = library.index.get(&pf.id).copied() else { continue };
        // roundabout tiles model the island as a HOLE — fill it with grass
        if let Some(def) = roads::prefab::by_id(&pf.id) {
            if def.island_r > 0.0 {
                let ir = (def.island_r * pf.scale * 0.96) as f32;
                let (ix, iz) = ((j.x - cx as f64) as f32, (j.z - cz as f64) as f32);
                let ring = 24;
                let mut disc = ScenePart {
                    positions: vec![Vec3::new(ix, 0.05, iz)],
                    indices: vec![],
                    color: [1.2, 1.2, 1.2, 1.0],
                    uv: vec![[(ix / 4.0) as f32, (iz / 4.0) as f32]],
                    tex: crate::textures::TEX_GRASS,
                };
                for k in 0..=ring {
                    let a = k as f32 / ring as f32 * std::f32::consts::TAU;
                    let x = ix + a.cos() * ir;
                    let z = iz + a.sin() * ir;
                    disc.positions.push(Vec3::new(x, 0.05, z));
                    disc.uv.push([x / 4.0, z / 4.0]);
                }
                for k in 1..=ring {
                    disc.indices.extend_from_slice(&[0, k as u32, (k + 1) as u32]);
                }
                island_discs.push(disc);
            }
        }
        let t = Mat4::translation((j.x - cx as f64) as f32, 0.012, (j.z - cz as f64) as f32);
        let r = Mat4::rot_y(pf.yaw as f32);
        let s = Mat4::scale(pf.scale as f32, pf.scale as f32, if pf.flip { -pf.scale as f32 } else { pf.scale as f32 });
        prop_draws.push(PropDraw {
            asset: idx,
            model: t.mul(&r).mul(&s),
            tex: crate::textures::TEX_ROAD_TRIM,
        });
    }

    // AI-lane direction fins (GRG `RoadLane` preview / `draw_lanes_editor`):
    // small flat "shark fin" triangles along every generated lane, oriented
    // with the travel direction so the traffic flow reads at a glance.
    if show_lanes && !lanes.is_empty() {
        append_lane_fins(&mut parts, curves, lanes, cx, cz, &mut tris);
    }

    // ground = GRID ONLY (Godot-editor style): no textured grass plane — the
    // dark background shows through and the grid gives the depth reference.
    // With the RoadTerrain3DConnector enabled the flat plane becomes a
    // heightfield: hills + GRG APPROXIMATE flattening under the roads.
    let radius = (((max_x - min_x) as f32).max((max_z - min_z) as f32) * 0.5 + 40.0).max(60.0);

    let mut ground: Vec<ScenePart> = Vec::new();
    for disc in island_discs {
        ground.push(disc);
    }

    let terrain_on = terrain.map(|t| t.enabled).unwrap_or(false);
    if terrain_on {
        let t = terrain.unwrap();
        let flatten = TerrainFlatten::new(t, curves);
        build_terrain_ground(&mut ground, t, &flatten, radius, cx, cz, &mut tris);
        if show_grid {
            build_grid(&mut ground, radius, cx, cz, &mut tris, Some((t, &flatten)));
        }
    } else if show_grid {
        build_grid(&mut ground, radius, cx, cz, &mut tris, None);
    }

    let prop_count = prop_draws.len();
    Scene {
        parts,
        props: prop_draws,
        ground,
        radius_m: radius,
        stats: SceneStats {
            curves: curves.len(),
            points: total_points,
            props: prop_count,
            tris,
            length_m: total_len,
        },
    }
}

// ---------------------------------------------------------------------------
// AI-lane fins (GRG draw_lanes preview)
// ---------------------------------------------------------------------------

/// Lane-color bucket: 0 = forward, 1 = oncoming, 2 = transition (dim gray).
fn lane_bucket(lane: &roads::lane::RoadLane) -> usize {
    if lane.transition {
        2
    } else if lane.side == 'F' {
        0
    } else {
        1
    }
}

/// GRG draw_lanes preview colors: #37d97a forward, #e8952f oncoming,
/// #8a8f98 transition tapers.
const FIN_COLORS: [[f32; 4]; 3] = [
    [0x37 as f32 / 255.0, 0xd9 as f32 / 255.0, 0x7a as f32 / 255.0, 1.0],
    [0xe8 as f32 / 255.0, 0x95 as f32 / 255.0, 0x2f as f32 / 255.0, 1.0],
    [0x8a as f32 / 255.0, 0x8f as f32 / 255.0, 0x98 as f32 / 255.0, 1.0],
];

fn fin_color(bucket: usize) -> [f32; 4] {
    FIN_COLORS[bucket]
}

/// Append flat "shark fin" triangles along every AI lane (one triangle
/// every ~6 m, base 0.9 m wide, apex 0.7 m up, swept back with the yaw).
/// Fins respect the lane polyline elevation (lane y). Only containers with
/// `generate_ai_lanes` + `draw_lanes_editor` are drawn, exactly like the
/// plugin's editor lane preview.
fn append_lane_fins(
    parts: &mut Vec<ScenePart>,
    curves: &[RoadCurve],
    lanes: &[roads::lane::RoadLane],
    cx: f32,
    cz: f32,
    tris: &mut usize,
) {
    const STEP: f64 = 6.0;
    let mut buckets: [ScenePart; 3] = [
        ScenePart { positions: vec![], indices: vec![], color: fin_color(0), uv: vec![], tex: -1 },
        ScenePart { positions: vec![], indices: vec![], color: fin_color(1), uv: vec![], tex: -1 },
        ScenePart { positions: vec![], indices: vec![], color: fin_color(2), uv: vec![], tex: -1 },
    ];
    for lane in lanes {
        let Some(c) = curves.get(lane.container) else {
            continue;
        };
        if !c.generate_ai_lanes || !c.draw_lanes_editor {
            continue;
        }
        let total = lane.length_m();
        if total < 2.0 {
            continue;
        }
        let b = lane_bucket(lane);
        let mut s = STEP * 0.5;
        while s < total {
            if let Some((p, yaw)) = lane.sample_at(s) {
                let (sy, cy) = (yaw as f32).sin_cos();
                let dir = Vec3::new(cy, 0.0, sy); // along travel
                let perp = Vec3::new(-sy, 0.0, cy); // right of travel
                let base = Vec3::new(p.x - cx, p.y + 0.04, p.z - cz);
                let a = base - perp * 0.45;
                let apex = base - dir * 0.35 + Vec3::new(0.0, 0.7, 0.0);
                let c2 = base + perp * 0.45;
                buckets[b].positions.extend_from_slice(&[a, apex, c2]);
            }
            s += STEP;
        }
    }
    for part in buckets {
        if part.positions.is_empty() {
            continue;
        }
        *tris += part.positions.len() / 3;
        parts.push(part);
    }
}

// ---------------------------------------------------------------------------
// Terrain heightfield (RoadTerrain3DConnector, GRG APPROXIMATE method)
// ---------------------------------------------------------------------------

/// One flattened road sample: planar position, road-surface elevation
/// (per-point `elev` interpolation) + half paved width.
struct RoadSample {
    x: f64,
    z: f64,
    y: f64,
    hw: f64,
}

/// Shared flatten context: road samples (8 m density, per-curve + per-point
/// `flatten_terrain` respected) in a spatial hash + the eased height query.
/// Built ONCE per scene build, used by both the heightfield and the grid.
struct TerrainFlatten {
    samples: Vec<RoadSample>,
    cell: f64,
    hash: std::collections::HashMap<(i64, i64), Vec<u32>>,
}

impl TerrainFlatten {
    fn new(t: &roads::terrain::TerrainState, curves: &[RoadCurve]) -> Self {
        let mut samples: Vec<RoadSample> = Vec::new();
        if t.flatten_under_roads {
            for c in curves {
                if !c.flatten_terrain || c.points.len() < 2 {
                    continue;
                }
                let n = c.points.len();
                let segs = if c.closed { n } else { n - 1 };
                for k in 0..segs {
                    let a = &c.points[k];
                    let b = &c.points[(k + 1) % n];
                    if !a.flatten_terrain || !b.flatten_terrain {
                        continue; // per-point opt-out (GRG RoadPoint.flatten_terrain)
                    }
                    let seg = roads::curve::segment_cubic(a, b);
                    let (sp, sc, seg_len) = roads::curve::flatten_cubic(&seg, 8.0f64.max(1.0));
                    if seg_len < 1e-9 {
                        continue;
                    }
                    let (ea, eb) = (a.elev, b.elev);
                    let (wa, wb) = (c.paved_width_at(k) / 2.0, c.paved_width_at((k + 1) % n) / 2.0);
                    for (j, p) in sp.iter().enumerate() {
                        let u = (sc[j] / seg_len).clamp(0.0, 1.0);
                        samples.push(RoadSample {
                            x: p.x,
                            z: p.y,
                            y: ea + (eb - ea) * u,
                            hw: wa + (wb - wa) * u,
                        });
                    }
                }
            }
        }
        // cell >= max reach so a 3x3 neighborhood query always covers every
        // sample that can influence the query point
        let mut reach = 8.0f64;
        for s in &samples {
            reach = reach.max(s.hw + t.edge_margin + t.edge_falloff);
        }
        let cell = reach.max(8.0);
        let mut hash: std::collections::HashMap<(i64, i64), Vec<u32>> =
            std::collections::HashMap::new();
        for (i, s) in samples.iter().enumerate() {
            hash.entry(((s.x / cell).floor() as i64, (s.z / cell).floor() as i64))
                .or_default()
                .push(i as u32);
        }
        Self { samples, cell, hash }
    }

    /// Flattened terrain height at a world (x, z): hills, snapped to the
    /// nearest road elevation inside half-width + edge_margin, smoothly
    /// eased back over edge_falloff (GRG APPROXIMATE method).
    fn height(&self, t: &roads::terrain::TerrainState, x: f64, z: f64) -> f64 {
        let hill = t.hill_height(x, z);
        if self.samples.is_empty() {
            return hill;
        }
        let (gx, gz) = ((x / self.cell).floor() as i64, (z / self.cell).floor() as i64);
        let mut best: Option<(f64, f64, f64)> = None; // (dist, road_y, half_width)
        for dx in -1i64..=1 {
            for dz in -1i64..=1 {
                if let Some(list) = self.hash.get(&(gx + dx, gz + dz)) {
                    for &i in list {
                        let s = &self.samples[i as usize];
                        let d = ((s.x - x).powi(2) + (s.z - z).powi(2)).sqrt();
                        let span = s.hw + t.edge_margin + t.edge_falloff;
                        if d <= span && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                            best = Some((d, s.y, s.hw));
                        }
                    }
                }
            }
        }
        let Some((d, ry, hw)) = best else {
            return hill;
        };
        let m = hw + t.edge_margin;
        if d <= m {
            ry + t.offset // snap under the road surface (no z-fighting)
        } else {
            // smooth ease t = 1 - ((d - m) / falloff), eased t²(3 - 2t)
            let u = ((d - m) / t.edge_falloff.max(0.01)).clamp(0.0, 1.0);
            let tt = 1.0 - u;
            let eased = tt * tt * (3.0 - 2.0 * tt);
            hill + (ry + t.offset - hill) * eased
        }
    }
}

/// Terrain ground: heightfield grid (spacing ~8 m, capped at 96x96 verts)
/// over the scene radius. Vertices are colored by height band with a muted
/// earth ramp (#2e3440 -> #4a5261) — NOT grass, the grid-only aesthetic.
fn build_terrain_ground(
    ground: &mut Vec<ScenePart>,
    t: &roads::terrain::TerrainState,
    flatten: &TerrainFlatten,
    radius: f32,
    cx: f32,
    cz: f32,
    tris: &mut usize,
) {
    // vertex grid
    let extent = radius * 1.2;
    let spacing = ((2.0 * extent) / 95.0).max(8.0);
    let n = (((2.0 * extent) / spacing).floor() as usize).clamp(2, 96);
    let step = (2.0 * extent) / n as f32;
    let stride = n + 1;
    let mut ys = vec![0.0f32; stride * stride];
    let (mut hmin, mut hmax) = (f32::INFINITY, f32::NEG_INFINITY);
    for j in 0..=n {
        for i in 0..=n {
            let x = (cx - extent + i as f32 * step) as f64;
            let z = (cz - extent + j as f32 * step) as f64;
            let y = flatten.height(t, x, z) as f32;
            ys[j * stride + i] = y;
            hmin = hmin.min(y);
            hmax = hmax.max(y);
        }
    }

    // height bands -> one single-color part per band (per-quad colors via
    // band subdivision; the raster shades up-facing quads uniformly)
    const LO: [f32; 3] = [0.1804, 0.2039, 0.2510]; // #2e3440
    const HI: [f32; 3] = [0.2902, 0.3216, 0.3804]; // #4a5261
    const BANDS: usize = 8;
    let mut band_parts: Vec<ScenePart> = (0..BANDS)
        .map(|b| {
            let f = b as f32 / (BANDS - 1) as f32;
            ScenePart {
                positions: vec![],
                indices: vec![],
                color: [
                    LO[0] + (HI[0] - LO[0]) * f,
                    LO[1] + (HI[1] - LO[1]) * f,
                    LO[2] + (HI[2] - LO[2]) * f,
                    1.0,
                ],
                uv: vec![],
                tex: -1,
            }
        })
        .collect();
    let hspan = (hmax - hmin).max(0.001);
    for j in 0..n {
        for i in 0..n {
            let y00 = ys[j * stride + i];
            let y10 = ys[j * stride + i + 1];
            let y01 = ys[(j + 1) * stride + i];
            let y11 = ys[(j + 1) * stride + i + 1];
            let avg = (y00 + y10 + y01 + y11) * 0.25;
            let b = ((((avg - hmin) / hspan) * (BANDS - 1) as f32).round() as usize).min(BANDS - 1);
            let x0 = -extent + i as f32 * step;
            let x1 = x0 + step;
            let z0 = -extent + j as f32 * step;
            let z1 = z0 + step;
            let part = &mut band_parts[b];
            let base = part.positions.len() as u32;
            part.positions.extend_from_slice(&[
                Vec3::new(x0, y00, z0),
                Vec3::new(x1, y10, z0),
                Vec3::new(x0, y01, z1),
                Vec3::new(x1, y11, z1),
            ]);
            part.indices
                .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }
    }
    for part in band_parts {
        if part.indices.is_empty() {
            continue;
        }
        *tris += part.indices.len() / 3;
        ground.push(part);
    }
}

/// Reference ground grid. With `flatten` (terrain enabled) the lines follow
/// the heightfield (sampled per segment corner, 3 cm above the terrain);
/// otherwise they stay flat at y = -0.02 exactly as the classic grid-only
/// environment.
fn build_grid(
    ground: &mut Vec<ScenePart>,
    radius: f32,
    cx: f32,
    cz: f32,
    tris: &mut usize,
    flatten: Option<(&roads::terrain::TerrainState, &TerrainFlatten)>,
) {
    let mut g = ScenePart {
        positions: vec![],
        indices: vec![],
        color: [0.42, 0.46, 0.52, 1.0],
        uv: vec![],
        tex: -1,
    };
    let spacing = if radius > 300.0 { 50.0 } else { 25.0 };
    let extent = radius * 1.6;
    let hw = if radius > 300.0 { 0.2 } else { 0.08 };

    let mut line_y = |a: Vec3, b: Vec3, g: &mut ScenePart| {
        let base = g.positions.len() as u32;
        let dir = (b - a).normalized();
        let side = Vec3::new(-dir.z, 0.0, dir.x) * hw;
        g.positions.push(a - side);
        g.positions.push(a + side);
        g.positions.push(b - side);
        g.positions.push(b + side);
        g.indices.extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    };

    if let Some((t, fl)) = flatten {
        // lines follow the heightfield: sample each segment corner so grid
        // and hills never intersect (cheap — same vertex lattice density)
        let mut k = -extent;
        while k <= extent {
            for (vertical, fixed) in [(true, k), (false, k)] {
                let mut s0 = -extent;
                while s0 < extent {
                    let s1 = (s0 + spacing).min(extent);
                    let (ax, az) = if vertical { (fixed, s0) } else { (s0, fixed) };
                    let (bx, bz) = if vertical { (fixed, s1) } else { (s1, fixed) };
                    let ya = fl.height(t, ax as f64, az as f64) as f32 + 0.03;
                    let yb = fl.height(t, bx as f64, bz as f64) as f32 + 0.03;
                    let a = Vec3::new(ax - cx, ya, az - cz);
                    let b = Vec3::new(bx - cx, yb, bz - cz);
                    line_y(a, b, &mut g);
                    if s1 >= extent {
                        break;
                    }
                    s0 = s1;
                }
            }
            if k >= extent {
                break;
            }
            k += spacing;
        }
    } else {
        let y = -0.02f32;
        let mut k = -extent;
        while k <= extent {
            for (a, b) in [
                (Vec3::new(k, y, -extent), Vec3::new(k, y, extent)),
                (Vec3::new(-extent, y, k), Vec3::new(extent, y, k)),
            ] {
                line_y(a, b, &mut g);
            }
            k += spacing;
        }
    }
    *tris += g.indices.len() / 3;
    ground.push(g);
}

/// Render the scene to RGBA8 (props drawn through the asset library,
/// road/ground parts through the texture library).
/// The raster work is split across one thread per horizontal band.
#[allow(clippy::too_many_arguments)]
pub fn render_scene(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    bg: [u8; 4],
    extra: &[PropDraw],
) -> Vec<u8> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16)
        .min(h / 120.max(1));
    if threads <= 1 {
        let mut frame = Frame::new(w, h, bg);
        draw_all(
            &mut frame, scene, library, textures, cam, bg, extra,
        );
        return frame.color;
    }
    let band = h.div_ceil(threads);
    let mut bands: Vec<(usize, usize)> = Vec::new(); // (y_base, band_h)
    let mut y0 = 0usize;
    while y0 < h {
        let bh = band.min(h - y0);
        bands.push((y0, bh));
        y0 += bh;
    }
    let results: Vec<Frame> = std::thread::scope(|s| {
        let handles: Vec<_> = bands
            .iter()
            .map(|&(yb, bh)| {
                s.spawn(move || {
                    let mut frame = Frame::band(w, bh, bg, yb, h);
                    draw_all(&mut frame, scene, library, textures, cam, bg, extra);
                    frame
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|hnd| hnd.join().expect("render band thread"))
            .collect()
    });
    // stitch bands back into one image
    let mut out = vec![0u8; w * h * 4];
    for (row, (yb, bh)) in bands.iter().enumerate() {
        let src = &results[row].color;
        let dst_off = yb * w * 4;
        out[dst_off..dst_off + bh * w * 4].copy_from_slice(&src[..bh * w * 4]);
    }
    out
}

/// Like render_scene, but can skip every GLB prop draw (the "props" toggle)
/// without cloning the scene.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_props(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    bg: [u8; 4],
    extra: &[PropDraw],
    show_props: bool,
) -> Vec<u8> {
    if show_props {
        render_scene(scene, library, textures, cam, w, h, bg, extra)
    } else {
        let empty: Vec<PropDraw> = Vec::new();
        let mut bare = scene.clone();
        bare.props.clear();
        render_scene(&bare, library, textures, cam, w, h, bg, if extra.is_empty() { &empty } else { extra })
    }
}

/// Draw every scene element into one frame (shared by the serial + band paths).
#[allow(clippy::too_many_arguments)]
fn draw_all(
    frame: &mut Frame,
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    bg: [u8; 4],
    extra: &[PropDraw],
) {
    // distance fog measured from the CAMERA: content near the orbit target
    // stays clear, the far roads fade into the background
    frame.set_fog(cam.dist + scene.radius_m * 0.55, cam.dist + scene.radius_m * 3.4, bg);
    let view = cam.view();
    let proj = cam.proj(frame.w as f32 / frame.h.max(1) as f32);
    let light = Vec3::new(0.4, 0.85, 0.3).normalized();
    let eye = cam.eye();
    for part in scene.ground.iter().chain(scene.parts.iter()) {
        let tex = textures.get(part.tex);
        let uvs = if tex.is_some() { Some(&part.uv[..]) } else { None };
        frame.draw_mesh(
            &part.positions,
            &part.indices,
            None,
            uvs,
            tex,
            &Mat4::IDENTITY,
            &view,
            &proj,
            eye,
            light,
            part.color,
        );
    }
    for pd in scene.props.iter().chain(extra.iter()) {
        let Some(entry) = library.entries.get(pd.asset) else {
            continue;
        };
        let tex = if pd.tex >= 0 { textures.get(pd.tex) } else { None };
        for prim in &entry.mesh.prims {
            let uvs = if tex.is_some() && !prim.uvs.is_empty() {
                Some(&prim.uvs[..])
            } else {
                None
            };
            frame.draw_mesh(
                &prim.positions,
                &prim.indices,
                Some(&prim.colors),
                uvs,
                tex,
                &pd.model,
                &view,
                &proj,
                eye,
                light,
                prim.color,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use roads::model::RoadPoint;

    fn two_point_curve() -> Vec<RoadCurve> {
        let mut c = RoadCurve::new("LaneTest");
        let mut a = RoadPoint::new(0.0, 0.0);
        a.lanes = 4;
        a.rev_lanes = 2;
        a.lane_width = 4.0;
        let mut b = RoadPoint::new(80.0, 0.0);
        b.lanes = 4;
        b.rev_lanes = 2;
        b.lane_width = 4.0;
        c.add_point(a);
        c.add_point(b);
        vec![c]
    }

    #[test]
    fn lanes_and_terrain_extend_scene() {
        let curves = two_point_curve();
        // tolerant loader: empty library when assets are not present
        let lib = AssetLibrary::load_dir(std::path::Path::new("../../assets"));
        let lanes = roads::lane::generate_lanes(&curves, 8.0);
        assert_eq!(lanes.len(), 4, "2 F + 2 R lanes on one segment");

        // lane fins add flat-color parts over the bare scene
        let bare = build_scene(&curves, &[], &[], &lib, false);
        let full = build_scene_full(&curves, &[], &[], &lanes, &lib, false, true, None);
        assert!(
            full.parts.len() > bare.parts.len(),
            "lane fins must add parts ({} vs {})",
            full.parts.len(),
            bare.parts.len()
        );
        let green = full
            .parts
            .iter()
            .any(|p| p.tex == -1 && (p.color[0] - FIN_COLORS[0][0]).abs() < 1e-4);
        assert!(green, "forward fins carry the #37d97a flat color");

        // terrain enabled grows the ground (heightfield) and stays untextured
        let mut terrain = roads::terrain::TerrainState::default();
        terrain.enabled = true;
        terrain.amplitude = 6.0;
        let flat = build_scene_full(&curves, &[], &[], &[], &lib, true, false, None);
        let hilly = build_scene_full(&curves, &[], &[], &[], &lib, true, false, Some(&terrain));
        let flat_ground: usize = flat.ground.iter().map(|g| g.positions.len()).sum();
        let hilly_ground: usize = hilly.ground.iter().map(|g| g.positions.len()).sum();
        assert!(
            hilly_ground > flat_ground,
            "terrain heightfield must grow the ground mesh ({} vs {})",
            hilly_ground,
            flat_ground
        );
        assert!(
            hilly.ground.iter().all(|g| g.tex == -1),
            "terrain stays untextured (grid-only aesthetic)"
        );
        // the classic wrapper keeps the flat behavior (no terrain, no fins)
        let wrapped = build_scene(&curves, &[], &[], &lib, true);
        assert_eq!(wrapped.ground.len(), flat.ground.len());
    }
}
