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
    /// per-vertex colors (when non-empty; overrides `color`) — used by the
    /// distance-faded grid
    pub vcolors: Vec<[f32; 4]>,
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
    build_scene_full_with_bg(
        curves,
        junctions,
        props,
        &[],
        library,
        show_grid,
        false,
        None,
        ENV_DAY.clear_rgb(),
    )
}

/// `build_scene` with an explicit backdrop color (u8 rgba): the ground-rim
/// fade target (pre-divided by the up-face lambert so the shaded rim lands
/// EXACTLY on the fog color). Pass the SAME color you render with so the
/// ground melts seamlessly into your background.
pub fn build_scene_with_bg(
    curves: &[RoadCurve],
    junctions: &[roads::junction::Junction],
    props: &[crate::props::PropInstance],
    library: &AssetLibrary,
    show_grid: bool,
    bg: [u8; 4],
) -> Scene {
    build_scene_full_with_bg(
        curves,
        junctions,
        props,
        &[],
        library,
        show_grid,
        false,
        None,
        [bg[0] as f32 / 255.0, bg[1] as f32 / 255.0, bg[2] as f32 / 255.0],
    )
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
    build_scene_full_with_bg(
        curves,
        junctions,
        props,
        lanes,
        library,
        show_grid,
        show_lanes,
        terrain,
        ENV_DAY.clear_rgb(),
    )
}

/// `build_scene_full` with an explicit backdrop (0..1 rgb): the ground-rim
/// fade target = the fog color you will render with.
#[allow(clippy::too_many_arguments)]
pub fn build_scene_full_with_bg(
    curves: &[RoadCurve],
    junctions: &[roads::junction::Junction],
    props: &[crate::props::PropInstance],
    lanes: &[roads::lane::RoadLane],
    library: &AssetLibrary,
    show_grid: bool,
    show_lanes: bool,
    terrain: Option<&roads::terrain::TerrainState>,
    backdrop: [f32; 3],
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
                vcolors: vec![],
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
                    vcolors: vec![],
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
                vcolors: vec![],
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
                    vcolors: vec![],
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

    // ground = the ENGINE TERRAIN ENVIRONMENT (yakui-app look copied in):
    // grass ramp ground melting into the sky haze, with the editor grid on
    // top. With the RoadTerrain3DConnector enabled the flat plane becomes a
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
        build_terrain_ground(&mut ground, t, &flatten, radius, cx, cz, &mut tris, backdrop);
        if show_grid {
            build_grid(&mut ground, radius, cx, cz, &mut tris, Some((t, &flatten)), backdrop);
        }
    } else {
        build_flat_grass(&mut ground, radius, &mut tris, backdrop);
        if show_grid {
            build_grid(&mut ground, radius, cx, cz, &mut tris, None, backdrop);
        }
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
        ScenePart { positions: vec![], indices: vec![], color: fin_color(0), vcolors: vec![], uv: vec![], tex: -1 },
        ScenePart { positions: vec![], indices: vec![], color: fin_color(1), vcolors: vec![], uv: vec![], tex: -1 },
        ScenePart { positions: vec![], indices: vec![], color: fin_color(2), vcolors: vec![], uv: vec![], tex: -1 },
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

/// Engine ground ramp (crates/engine terrain.rs ground_color): grass -> dry
/// grass -> dirt -> rock -> sand by normalized height. Copied from the main
/// engine so the toolkit viewport shows the SAME terrain environment.
const GRASS_RAMP: [[f32; 3]; 6] = [
    [0.42, 0.60, 0.28], // low grass
    [0.47, 0.64, 0.31], // grass
    [0.55, 0.61, 0.32], // dry grass
    [0.60, 0.55, 0.38], // dirt
    [0.54, 0.51, 0.46], // rock
    [0.74, 0.70, 0.56], // sand / road margin
];

/// Flat base grass (ramp band 1) — the color the grid lines melt into.
const GRASS_BASE: [f32; 3] = [0.47, 0.64, 0.31];

/// Terrain ground: heightfield grid (spacing ~8 m, capped at 96x96 verts)
/// over the scene radius. Vertices are colored by height band with the
/// engine grass ramp; the outer ring fades into the fog/horizon color so
/// the heightfield edge never shows a hard seam against the sky.
#[allow(clippy::too_many_arguments)]
fn build_terrain_ground(
    ground: &mut Vec<ScenePart>,
    t: &roads::terrain::TerrainState,
    flatten: &TerrainFlatten,
    radius: f32,
    cx: f32,
    cz: f32,
    tris: &mut usize,
    horizon: [f32; 3],
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

    // height bands -> one part per engine-ramp band; per-vertex colors fade
    // the outer ring into the fog/horizon so the edge melts into the sky
    const BANDS: usize = GRASS_RAMP.len();
    let mut band_parts: Vec<ScenePart> = (0..BANDS)
        .map(|b| ScenePart {
            positions: vec![],
            indices: vec![],
            color: [GRASS_RAMP[b][0], GRASS_RAMP[b][1], GRASS_RAMP[b][2], 1.0],
            vcolors: vec![],
            uv: vec![],
            tex: -1,
        })
        .collect();
    let inv = 1.0 / up_shade();
    let rim = [horizon[0] * inv, horizon[1] * inv, horizon[2] * inv];
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
            // per-vertex color: band color -> rim haze by normalized
            // distance from the center (only the outer ring moves)
            let vc = |x: f32, z: f32| -> [f32; 4] {
                let t = (x.abs() / extent).max(z.abs() / extent);
                let f = smoothstep(0.72, 1.0, t);
                [
                    GRASS_RAMP[b][0] + (rim[0] - GRASS_RAMP[b][0]) * f,
                    GRASS_RAMP[b][1] + (rim[1] - GRASS_RAMP[b][1]) * f,
                    GRASS_RAMP[b][2] + (rim[2] - GRASS_RAMP[b][2]) * f,
                    1.0,
                ]
            };
            let part = &mut band_parts[b];
            let base = part.positions.len() as u32;
            part.positions.extend_from_slice(&[
                Vec3::new(x0, y00, z0),
                Vec3::new(x1, y10, z0),
                Vec3::new(x0, y01, z1),
                Vec3::new(x1, y11, z1),
            ]);
            part.vcolors
                .extend_from_slice(&[vc(x0, z0), vc(x1, z0), vc(x0, z1), vc(x1, z1)]);
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

/// Engine-day environment (copied from the main engine / yakui-app): the
/// pale-haze clear color (also the fog + sky-key color) and the sky gradient
/// (zenith -> horizon) the post-pass paints above the horizon line.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    /// flat clear/fog/sky-key color (u8 rgba)
    pub clear: [u8; 4],
    /// sky gradient zenith (0..1 rgb)
    pub zenith: [f32; 3],
    /// sky gradient horizon (0..1 rgb)
    pub horizon: [f32; 3],
}

impl Env {
    /// clear color as 0..1 rgb — the ground-rim fade target (fog color).
    pub fn clear_rgb(&self) -> [f32; 3] {
        [
            self.clear[0] as f32 / 255.0,
            self.clear[1] as f32 / 255.0,
            self.clear[2] as f32 / 255.0,
        ]
    }
}

/// The engine's day look: pale haze fog + zenith-blue sky over grass terrain.
pub const ENV_DAY: Env = Env {
    clear: [178, 199, 220, 255],
    zenith: [92.0 / 255.0, 138.0 / 255.0, 198.0 / 255.0],
    horizon: [199.0 / 255.0, 214.0 / 255.0, 229.0 / 255.0],
};

/// The raster multiplies every fragment by `0.38 + 0.62 * (n·l)`; up-facing
/// quads land on this exact factor. Colors that must render as exactly the
/// backdrop (faded line ends, stage-disc rim) are pre-divided by it.
fn up_shade() -> f32 {
    let l = Vec3::new(0.4, 0.85, 0.3).normalized();
    0.38 + 0.62 * l.y
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a).max(1e-5)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Editor-grade reference ground grid over the engine terrain environment:
/// - adaptive spacing (10 / 25 / 50 / 100 m by scene radius), major line
///   every 4th minor
/// - origin axes tinted X-red / Z-blue like the viewport axis gizmo
/// - bright clear lines that melt into the grass they sit on via per-vertex
///   colors (the CPU raster has no MSAA — sub-pixel quads break into
///   dashes, so far segments fade before they can alias)
/// - with terrain the lines follow the heightfield; flat 3 cm above the
///   grass plane otherwise
fn build_grid(
    ground: &mut Vec<ScenePart>,
    radius: f32,
    cx: f32,
    cz: f32,
    tris: &mut usize,
    flatten: Option<(&roads::terrain::TerrainState, &TerrainFlatten)>,
    horizon: [f32; 3],
) {
    let spacing = if radius <= 120.0 {
        10.0
    } else if radius <= 300.0 {
        25.0
    } else if radius <= 800.0 {
        50.0
    } else {
        100.0
    };
    let major_every = 4i64;
    // grid extent matches the GROUND extent exactly (heightfield 1.2r when
    // terrain rides along, grass fan 1.6r flat) — lines never float past it
    let ground_extent = if flatten.is_some() { radius * 1.2 } else { radius * 1.6 };
    let n = (ground_extent / spacing).floor().max(2.0) as i64;
    let extent = n as f32 * spacing;

    // the fade target replicates the ground's own rim melt (grass -> haze),
    // so line ends land exactly on the ground color at every distance.
    // distance shape matches the ground mesh: radial for the flat grass fan,
    // Chebyshev (square) for the terrain heightfield square
    let inv = 1.0 / up_shade();
    let rim = [horizon[0] * inv, horizon[1] * inv, horizon[2] * inv];
    let rim_from = if flatten.is_some() { 0.72f32 } else { 0.35f32 };
    let radial = flatten.is_none();
    let ground_col = move |x: f32, z: f32| -> [f32; 3] {
        let t = if radial {
            (x * x + z * z).sqrt() / ground_extent
        } else {
            (x.abs() / ground_extent).max(z.abs() / ground_extent)
        };
        let f = smoothstep(rim_from, 1.0, t);
        [
            GRASS_BASE[0] + (rim[0] - GRASS_BASE[0]) * f,
            GRASS_BASE[1] + (rim[1] - GRASS_BASE[1]) * f,
            GRASS_BASE[2] + (rim[2] - GRASS_BASE[2]) * f,
        ]
    };

    // line classes: (color, half width, fade start, fade end) — fade radii
    // in meters from the origin; everything melts into the grass by the edge
    let minor = ([0.82, 0.86, 0.74], 0.10, extent * 0.55, extent * 1.00);
    let major = ([0.94, 0.96, 0.86], 0.26, extent * 0.62, extent * 1.00);
    let axis_x = ([0.85, 0.26, 0.22], 0.38, extent * 0.70, extent * 1.00);
    let axis_z = ([0.25, 0.45, 0.86], 0.38, extent * 0.70, extent * 1.00);
    let classes = [minor, major, axis_x, axis_z];
    let mut parts: Vec<ScenePart> = classes
        .iter()
        .map(|(c, ..)| ScenePart {
            positions: vec![],
            indices: vec![],
            color: [c[0], c[1], c[2], 1.0],
            vcolors: vec![],
            uv: vec![],
            tex: -1,
        })
        .collect();

    // per-vertex color at a line vertex: line color -> local ground color
    // (which itself fades to the rim haze) by distance from the origin
    let vcol = |x: f32, z: f32, cls: usize| -> [f32; 4] {
        let (c, _, fs, fe) = classes[cls];
        let d = (x * x + z * z).sqrt();
        let f = 1.0 - smoothstep(fs, fe, d);
        let g = ground_col(x, z);
        [
            g[0] + (c[0] - g[0]) * f,
            g[1] + (c[1] - g[1]) * f,
            g[2] + (c[2] - g[2]) * f,
            1.0,
        ]
    };

    // one segment quad (a -> b) with per-vertex colors at the two ends
    let seg = |p: &mut ScenePart, a: Vec3, b: Vec3, ca: [f32; 4], cb: [f32; 4], hw: f32| {
        let dir = (b - a).normalized();
        let side = Vec3::new(-dir.z, 0.0, dir.x) * hw;
        let base = p.positions.len() as u32;
        p.positions.push(a - side);
        p.positions.push(a + side);
        p.positions.push(b - side);
        p.positions.push(b + side);
        p.vcolors.extend_from_slice(&[ca, ca, cb, cb]);
        p.indices
            .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
    };

    // height of a grid vertex (terrain-following or 3 cm over the grass)
    let height = |x: f32, z: f32| -> f32 {
        match flatten {
            Some((t, fl)) => fl.height(t, (x + cx) as f64, (z + cz) as f64) as f32 + 0.03,
            None => -0.01,
        }
    };

    // lines at i * spacing; i == 0 -> origin axis, i % 4 == 0 -> major
    for i in -n..=n {
        let k = i as f32 * spacing;
        // class index per direction: fixed-x lines (varying z) carry the Z
        // axis tint (classes[3]), fixed-z lines (varying x) the X axis tint
        // (classes[2]); i % 4 == 0 -> major (1), else minor (0)
        let (cls_z, cls_x) = if i == 0 {
            (3usize, 2usize)
        } else if i % major_every == 0 {
            (1usize, 1usize)
        } else {
            (0usize, 0usize)
        };
        let mut s0 = -extent;
        while s0 < extent {
            let s1 = (s0 + spacing).min(extent);
            // line along z (fixed x = k): the Z-axis class tint
            {
                let p = &mut parts[cls_z];
                let hw = classes[cls_z].1;
                let a = Vec3::new(k, height(k, s0), s0);
                let b = Vec3::new(k, height(k, s1), s1);
                seg(p, a, b, vcol(k, s0, cls_z), vcol(k, s1, cls_z), hw);
            }
            // line along x (fixed z = k): the X-axis class tint
            {
                let p = &mut parts[cls_x];
                let hw = classes[cls_x].1;
                let a = Vec3::new(s0, height(s0, k), k);
                let b = Vec3::new(s1, height(s1, k), k);
                seg(p, a, b, vcol(s0, k, cls_x), vcol(s1, k, cls_x), hw);
            }
            if s1 >= extent {
                break;
            }
            s0 = s1;
        }
    }
    for p in parts {
        *tris += p.indices.len() / 3;
        if !p.indices.is_empty() {
            ground.push(p);
        }
    }
}

/// Flat engine-terrain ground (terrain connector OFF): a soft grass fan
/// sitting just under the roads, melting into the sky haze at the rim —
/// the main engine's flat-map look copied into the toolkit.
fn build_flat_grass(
    ground: &mut Vec<ScenePart>,
    radius: f32,
    tris: &mut usize,
    horizon: [f32; 3],
) {
    let inv = 1.0 / up_shade();
    let rim = [horizon[0] * inv, horizon[1] * inv, horizon[2] * inv];
    let extent = radius * 1.6;
    const RINGS: usize = 3;
    const SEG: usize = 96;
    let mut fan = ScenePart {
        positions: vec![Vec3::new(0.0, -0.04, 0.0)],
        indices: vec![],
        color: [GRASS_BASE[0], GRASS_BASE[1], GRASS_BASE[2], 1.0],
        vcolors: vec![[GRASS_BASE[0], GRASS_BASE[1], GRASS_BASE[2], 1.0]],
        uv: vec![],
        tex: -1,
    };
    let ring_col = |r: f32| -> [f32; 4] {
        let f = smoothstep(0.35, 1.0, r / extent);
        [
            GRASS_BASE[0] + (rim[0] - GRASS_BASE[0]) * f,
            GRASS_BASE[1] + (rim[1] - GRASS_BASE[1]) * f,
            GRASS_BASE[2] + (rim[2] - GRASS_BASE[2]) * f,
            1.0,
        ]
    };
    let mut ring_base: Vec<u32> = Vec::new();
    for k in 0..RINGS {
        let r = extent * (k + 1) as f32 / RINGS as f32;
        let col = ring_col(r);
        let b = fan.positions.len() as u32;
        ring_base.push(b);
        for s in 0..=SEG {
            let a = s as f32 / SEG as f32 * std::f32::consts::TAU;
            fan.positions.push(Vec3::new(a.cos() * r, -0.04, a.sin() * r));
            fan.vcolors.push(col);
        }
    }
    // center fan out to the first ring
    for s in 1..=SEG {
        fan.indices
            .extend_from_slice(&[0, ring_base[0] + s as u32 - 1, ring_base[0] + s as u32]);
    }
    // strips between consecutive rings
    for k in 0..RINGS - 1 {
        let (b0, b1) = (ring_base[k], ring_base[k + 1]);
        for s in 0..SEG {
            let (i0, i1) = (b0 + s as u32, b0 + s as u32 + 1);
            let (j0, j1) = (b1 + s as u32, b1 + s as u32 + 1);
            fan.indices.extend_from_slice(&[i0, j0, i1, i1, j0, j1]);
        }
    }
    *tris += fan.indices.len() / 3;
    ground.push(fan);
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

/// `render_scene` against a full day environment: pale-haze clear + fog,
/// then the engine sky gradient painted above the horizon line. This is the
/// main engine's (yakui-app) 3D environment copied into the toolkit.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_env(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: Env,
    extra: &[PropDraw],
) -> Vec<u8> {
    let mut rgba = render_scene(scene, library, textures, cam, w, h, env.clear, extra);
    paint_sky(&mut rgba, w, h, cam.pitch, env);
    rgba
}

/// `render_scene_props` with the day environment (sky + haze).
#[allow(clippy::too_many_arguments)]
pub fn render_scene_props_env(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: Env,
    extra: &[PropDraw],
    show_props: bool,
) -> Vec<u8> {
    if show_props {
        render_scene_env(scene, library, textures, cam, w, h, env, extra)
    } else {
        let empty: Vec<PropDraw> = Vec::new();
        let mut bare = scene.clone();
        bare.props.clear();
        render_scene_env(
            &bare,
            library,
            textures,
            cam,
            w,
            h,
            env,
            if extra.is_empty() { &empty } else { extra },
        )
    }
}

/// Engine sky post-pass (yakui-app `paint_sky` verbatim): replace clear-color
/// pixels ABOVE the horizon with the zenith->horizon gradient. The camera
/// pitch and the vertical FOV (0.9 rad, gdmath OrbitCam) give the horizon
/// scanline; below it the fog already blends the ground into the haze.
fn paint_sky(rgba: &mut [u8], w: usize, h: usize, pitch: f32, env: Env) {
    const FOV_V: f32 = 0.9;
    let t = pitch.tan() / (FOV_V / 2.0).tan();
    let y_h = ((h as f32) / 2.0 * (1.0 - t)).round();
    let y_h = (y_h as i32).clamp(0, h as i32) as usize;
    if y_h == 0 {
        return;
    }
    let zen = [env.zenith[0] * 255.0, env.zenith[1] * 255.0, env.zenith[2] * 255.0];
    let hor = [env.horizon[0] * 255.0, env.horizon[1] * 255.0, env.horizon[2] * 255.0];
    for y in 0..y_h {
        let f = (y as f32 / y_h as f32).powf(0.8);
        let r = (zen[0] + (hor[0] - zen[0]) * f) as u8;
        let g = (zen[1] + (hor[1] - zen[1]) * f) as u8;
        let b = (zen[2] + (hor[2] - zen[2]) * f) as u8;
        let row = y * w * 4;
        for x in 0..w {
            let i = row + x * 4;
            let is_bg = (rgba[i] as i32 - env.clear[0] as i32).abs() <= 4
                && (rgba[i + 1] as i32 - env.clear[1] as i32).abs() <= 4
                && (rgba[i + 2] as i32 - env.clear[2] as i32).abs() <= 4;
            if is_bg {
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
            }
        }
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
        let cols = if part.vcolors.is_empty() {
            None
        } else {
            Some(&part.vcolors[..])
        };
        frame.draw_mesh(
            &part.positions,
            &part.indices,
            cols,
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

        // terrain enabled grows the ground (heightfield) and stays untextured.
        // compared with the grid OFF so the (per-mode clamped) editor grid
        // doesn't skew the raw vertex counts
        let mut terrain = roads::terrain::TerrainState::default();
        terrain.enabled = true;
        terrain.amplitude = 6.0;
        let flat = build_scene_full(&curves, &[], &[], &[], &lib, false, false, None);
        let hilly = build_scene_full(&curves, &[], &[], &[], &lib, false, false, Some(&terrain));
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
        let flat_grid = build_scene_full(&curves, &[], &[], &[], &lib, true, false, None);
        let wrapped = build_scene(&curves, &[], &[], &lib, true);
        assert_eq!(wrapped.ground.len(), flat_grid.ground.len());
    }
}
