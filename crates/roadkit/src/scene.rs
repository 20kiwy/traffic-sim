//! 3D scene assembly + rendering.
//!
//! Mirrors the godot-road-generator preview: procedural road meshes
//! (roads::mesh3d) with GLB prop assets joined along the roads
//! (vehicle / milestone / tree / streetlight / junctionMarker).
//! Scene space = METERS, x east, y up, z south.

use crate::library::AssetLibrary;
use crate::raster::{
    box_downsample_f32, EnvCompose, Frame, ShadeMode, ShadowData,
};
use crate::textures::TextureLibrary;
use engine::atmosphere::{FogLut, SkyLut};
use gdmath::vec3::Vec3;
use gdmath::Mat4;
use roads::junction::{marking_parts_for, mesh_parts_for};
use std::sync::Arc;

/// The bbox-centre recentering `build_scene` applies (public so CLI/app
/// cameras can aim at the SAME shifted coordinates — a focus point in
/// curve space must become `focus - (cx, cz)` in scene space).
pub fn scene_recenter_full(curves: &[RoadCurve]) -> (f32, f32, usize, f64, (f64, f64, f64, f64)) {
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
    (cx, cz, total_points, total_len, (min_x, min_z, max_x, max_z))
}

/// Compact form used by cameras (centre + stats only).
pub fn scene_recenter(curves: &[RoadCurve]) -> (f32, f32, usize, f64) {
    let (cx, cz, tp, tl, _) = scene_recenter_full(curves);
    (cx, cz, tp, tl)
}

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
    /// ground plane (grass fan / terrain heightfield)
    pub ground: Vec<ScenePart>,
    /// the bbox-centre recentering subtracted from EVERY part/prop
    /// position (`scene = doc - origin`). Document-space overlays (RoadPoint
    /// gizmos, signal heads, cursor pick) subtract this before projecting so
    /// they stay glued to the meshes; un-projecting adds it back.
    pub origin: [f32; 2],
    pub radius_m: f32,
    /// meters: the ground's reach from the origin (heightfield 1.2r when
    /// the terrain connector rides along, grass fan 1.6r flat) — viewport
    /// reference lines never float past it
    pub ground_extent: f32,
    /// ground height sampler for overlays (None → flat 0-height ground)
    pub sampler: Option<GroundSampler>,
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
    build_scene_enriched(
        curves, junctions, props, lanes, library, show_grid, show_lanes,
        terrain, backdrop, &[],
    )
}

/// `build_scene_full_with_bg` + world-space enrichment geometry
/// (procedural buildings — `buildings::blocks_for_sites`), appended to
/// the road mesh parts with the same recentering offset.
#[allow(clippy::too_many_arguments)]
pub fn build_scene_enriched(
    curves: &[RoadCurve],
    junctions: &[roads::junction::Junction],
    props: &[crate::props::PropInstance],
    lanes: &[roads::lane::RoadLane],
    library: &AssetLibrary,
    show_grid: bool,
    show_lanes: bool,
    terrain: Option<&roads::terrain::TerrainState>,
    backdrop: [f32; 3],
    extras: &[roads::mesh3d::MeshPart],
) -> Scene {
    let _ = library; // props carry their own resolved indices
    // bbox of all curve points
    let (mut min_x, mut min_z) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    let (cx, cz, total_points, total_len, (min_x, min_z, max_x, max_z)) = scene_recenter_full(curves);

    // road meshes (textured)
    let mut parts: Vec<ScenePart> = Vec::new();
    let mut tris = 0usize;
    for c in curves {
        // per-curve assembly: build_parts covers surface + gutters +
        // piers; R10 adds bridge formwork (railing/abutments) and the
        // TANROADS roadside drains. wasm CPU budget: coarser road sampling
        // halves the per-frame vertex transform cost (the software raster
        // re-transforms every vertex every frame — 4 m sampling put web
        // closeups at ~110 ms/frame).
        let mut mesh: Vec<roads::mesh3d::MeshPart> = build_parts(
            c,
            MeshQuality {
                markings: true,
                #[cfg(target_arch = "wasm32")]
                sample_m: 8.0,
                #[cfg(not(target_arch = "wasm32"))]
                sample_m: 4.0,
            },
        );
        mesh.extend(roads::mesh3d::railing_parts(c));
        mesh.extend(roads::mesh3d::abutment_parts(c));
        mesh.extend(roads::mesh3d::drain_parts(c));
        // R12 RoadArchitect systems: GSDTunnel vault, suspension/semi-arch
        // bridge kits and the median cable barrier (all flag-gated — each
        // returns an empty vec when its RoadCurve flag is off)
        mesh.extend(roads::mesh3d::tunnel_parts(c, MeshQuality { markings: true, sample_m: 4.0 }));
        mesh.extend(roads::mesh3d::suspension_parts(c));
        mesh.extend(roads::mesh3d::arch_parts(c));
        mesh.extend(roads::mesh3d::cable_barrier_parts(c));
        for part in mesh {
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

    // R11 enrichment (buildings etc.) — same recentering as the road parts
    for part in extras {
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
            indices: part.indices.clone(),
            color: part.color,
            vcolors: vec![],
            uv: part.uv.clone(),
            tex: part.tex,
        });
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

    // The reference grid is NOT baked into the scene anymore: it is drawn
    // as a screen-space overlay by the viewport (the TerraZone look — thin
    // faint metre lines that never alias or blob). The scene only carries
    // the ground extent + height sampler the overlay needs.
    let _ = show_grid;
    let terrain_on = terrain.map(|t| t.enabled).unwrap_or(false);
    let ground_extent = if terrain_on { radius * 1.2 } else { radius * 1.6 };
    let mut sampler: Option<GroundSampler> = None;
    if terrain_on {
        let t = terrain.unwrap();
        let flatten = TerrainFlatten::new(t, curves);
        build_terrain_ground(&mut ground, t, &flatten, radius, cx, cz, &mut tris, backdrop);
        sampler = Some(GroundSampler {
            flatten,
            t: terrain.cloned(),
            cx,
            cz,
        });
    } else {
        build_flat_grass(&mut ground, radius, &mut tris, backdrop);
    }

    let prop_count = prop_draws.len();
    Scene {
        parts,
        props: prop_draws,
        ground,
        origin: [cx, cz],
        radius_m: radius,
        ground_extent,
        sampler,
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
#[derive(Clone)]
struct RoadSample {
    x: f64,
    z: f64,
    y: f64,
    hw: f64,
}

/// Shared flatten context: road samples (8 m density, per-curve + per-point
/// `flatten_terrain` respected) in a spatial hash + the eased height query.
/// Built ONCE per scene build, used by both the heightfield and the grid.
#[derive(Clone)]
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
    ///
    /// R15 fix: where several roads compete (grade-separated interchanges),
    /// the ground follows the LOWEST in-range road envelope, not the nearest
    /// sample — the nearest heuristic let a descending flyover ramp lift the
    /// ground ABOVE the at-grade underpass road, burying it (its markings
    /// then z-fought through the ground as a fan of dashes).
    fn height(&self, t: &roads::terrain::TerrainState, x: f64, z: f64) -> f64 {
        let hill = t.hill_height(x, z);
        if self.samples.is_empty() {
            return hill;
        }
        let (gx, gz) = ((x / self.cell).floor() as i64, (z / self.cell).floor() as i64);
        // lowest in-range road envelope: (road_y, half_width, falloff dist)
        let mut best: Option<(f64, f64, f64)> = None;
        for dx in -1i64..=1 {
            for dz in -1i64..=1 {
                if let Some(list) = self.hash.get(&(gx + dx, gz + dz)) {
                    for &i in list {
                        let s = &self.samples[i as usize];
                        let d = ((s.x - x).powi(2) + (s.z - z).powi(2)).sqrt();
                        let span = s.hw + t.edge_margin + t.edge_falloff;
                        if d <= span && best.map(|(by, _, _)| s.y < by).unwrap_or(true) {
                            best = Some((s.y, s.hw, d));
                        }
                    }
                }
            }
        }
        let Some((ry, hw, d)) = best else {
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

/// Scene-local ground height sampler carried on the built [`Scene`] so
/// viewport overlays (the TerraZone-style ground grid) can hug the SAME
/// surface the heightfield rendered: hills snapped to road elevations.
#[derive(Clone)]
pub struct GroundSampler {
    flatten: TerrainFlatten,
    t: Option<roads::terrain::TerrainState>,
    cx: f32,
    cz: f32,
}

impl GroundSampler {
    /// Scene-local `(x, z)` (the space all scene mesh positions live in)
    /// → ground height in meters (y up), roads flattened in.
    pub fn height(&self, x: f32, z: f32) -> f32 {
        match &self.t {
            Some(t) => self.flatten.height(t, (x + self.cx) as f64, (z + self.cz) as f64) as f32,
            None => 0.0,
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

/// Egregoria terrain blending: sand near the waterline, seabed below.
/// (heightmap.frag.wgsl: `mix(sand_col, c, smoothstep(-5, 0, z))`,
/// `mix(sea_col, c, smoothstep(-25, -20, z))` — our coast constants.)
const SAND_COL: [f32; 3] = [0.76, 0.71, 0.55];
const SEA_COL: [f32; 3] = [0.13, 0.20, 0.26];

/// Mean linear-ish albedo of the generated `terrain_grass.raw` texture
/// (assets/textures_src/gen_env_textures.py targets rgb ≈ 101,126,58).
/// Band colors are pre-DIVIDED by this so `tint × texel ≈ band color`.
const GRASS_TEX_MEAN: [f32; 3] = [0.396, 0.494, 0.227];

/// Tint = ramp color normalized by the grass texture mean (so the product
/// lands on the intended ramp color), clamped to a sane range.
fn grass_tint(c: [f32; 3]) -> [f32; 4] {
    [
        (c[0] / GRASS_TEX_MEAN[0]).clamp(0.2, 4.0),
        (c[1] / GRASS_TEX_MEAN[1]).clamp(0.2, 4.0),
        (c[2] / GRASS_TEX_MEAN[2]).clamp(0.2, 4.0),
        1.0,
    ]
}

/// Egregoria sand/sea blend by world height (smoothsteps at -5..0 and
/// -25..-20 m; a no-op above the waterline).
fn coast_blend(c: [f32; 3], y: f32) -> [f32; 3] {
    if y > -0.5 {
        return c; // flats sit at ~0 — no beach tint without a waterline
    }
    let s = smoothstep(-5.0, 0.0, y);
    let mut c = [SAND_COL[0] + (c[0] - SAND_COL[0]) * s, SAND_COL[1] + (c[1] - SAND_COL[1]) * s, SAND_COL[2] + (c[2] - SAND_COL[2]) * s];
    let s2 = smoothstep(-25.0, -20.0, y);
    c = [SEA_COL[0] + (c[0] - SEA_COL[0]) * s2, SEA_COL[1] + (c[1] - SEA_COL[1]) * s2, SEA_COL[2] + (c[2] - SEA_COL[2]) * s2];
    c
}

/// Terrain ground: heightfield grid (spacing ~8 m, capped at 96x96 verts)
/// over the scene radius. R15 Egregoria look: the heightfield is TEXTURED
/// with the procedural grass albedo (TEX_TERRAIN_GRASS, world-space UV at
/// 200 m tiling) tinted per height band by the engine grass ramp, blended
/// to sand/sea below the waterline; the outer ring still fades into the
/// haze so the heightfield edge never shows a hard seam against the sky.
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

    // height bands -> one part per engine-ramp band; the band color is the
    // per-vertex TINT over the grass texture; the rim fades into the haze
    const BANDS: usize = GRASS_RAMP.len();
    let mut band_parts: Vec<ScenePart> = (0..BANDS)
        .map(|b| ScenePart {
            positions: vec![],
            indices: vec![],
            color: [1.0, 1.0, 1.0, 1.0],
            vcolors: vec![],
            uv: vec![],
            tex: crate::textures::TEX_TERRAIN_GRASS,
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
            // per-vertex color: band tint (over the grass texture) + coast
            // blend + rim haze by normalized distance from the center
            let vc = |x: f32, z: f32, y: f32| -> [f32; 4] {
                let c = coast_blend(GRASS_RAMP[b], y);
                let t = (x.abs() / extent).max(z.abs() / extent);
                let f = smoothstep(0.72, 1.0, t);
                grass_tint([
                    c[0] + (rim[0] - c[0]) * f,
                    c[1] + (rim[1] - c[1]) * f,
                    c[2] + (rim[2] - c[2]) * f,
                ])
            };
            let part = &mut band_parts[b];
            let base = part.positions.len() as u32;
            part.positions.extend_from_slice(&[
                Vec3::new(x0, y00, z0),
                Vec3::new(x1, y10, z0),
                Vec3::new(x0, y01, z1),
                Vec3::new(x1, y11, z1),
            ]);
            // world-space grass UV (200 m tiles, Egregoria wpos.xy/200)
            part.uv.extend_from_slice(&[
                [(x0 + cx) / 200.0, (z0 + cz) / 200.0],
                [(x1 + cx) / 200.0, (z0 + cz) / 200.0],
                [(x0 + cx) / 200.0, (z1 + cz) / 200.0],
                [(x1 + cx) / 200.0, (z1 + cz) / 200.0],
            ]);
            part.vcolors
                .extend_from_slice(&[vc(x0, z0, y00), vc(x1, z0, y10), vc(x0, z1, y01), vc(x1, z1, y11)]);
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

/// The engine's day look: warm pale haze — the R13 grade. The old zenith
/// was a saturated blue (92,138,198) the user asked to see gone; the sky
/// now melts from a soft grey-blue into a warm haze, matching the fog.
pub const ENV_DAY: Env = Env {
    clear: [186, 192, 196, 255],
    zenith: [126.0 / 255.0, 148.0 / 255.0, 172.0 / 255.0],
    horizon: [214.0 / 255.0, 214.0 / 255.0, 206.0 / 255.0],
};

// ---------------------------------------------------------------------------
// R15 — the Egregoria 3D environment (engine remake)
// ---------------------------------------------------------------------------

/// The full 3D environment state, ported from the Egregoria engine.
/// Owns the sky LUT (rebuilt when the sun moves — i.e. when the time of
/// day changes), the analytic sun, and the render-time environment flags.
/// Passed by reference to the render entry; cheap to clone around threads.
#[derive(Clone)]
pub struct Environment {
    /// time of day in seconds (engine::sun day model, day starts 08:00)
    pub time_of_day: f64,
    /// normalized direction TOWARD the sun (y up)
    pub sun: Vec3,
    /// linear sun color × intensity (can exceed 1 at noon)
    pub sun_col: [f32; 3],
    /// single-scattering sky LUT (lat-long, 256×128)
    pub sky: Arc<SkyLut>,
    /// sea/water surface height (y). None = no water plane.
    pub water_level: Option<f32>,
    /// draw 10 m / 50 m elevation contour lines on the ground
    pub contours: bool,
    /// render the sun shadow map (single cascade)
    pub shadows: bool,
    /// wave/animation clock (static in still renders; the app may advance it)
    pub env_time: f32,
    /// Egregoria atmosphere: sky LUT backdrop + aerial-perspective fog.
    /// OFF = the clean EDITOR look — flat backdrop, no fog, unlit floor
    /// (the editor grid overlay then covers the whole ground).
    pub atmo: bool,
    /// flat backdrop color (sRGB 0..1 rgb) used when `atmo` is off — also
    /// the unlit editor floor color, so floor and sky melt into one paper.
    pub backdrop: [f32; 3],
}

impl Environment {
    /// Build the environment for a time of day (seconds) and flags.
    /// The sky LUT is rebuilt here (the only expensive step, ~33k
    /// atmosphere ray-marches — done once per sun change, not per frame).
    pub fn new(
        time_of_day: f64,
        shadows: bool,
        contours: bool,
        water_level: Option<f32>,
        atmo: bool,
        backdrop: [f32; 3],
    ) -> Self {
        let dir = engine::sun::sun_dir(time_of_day).normalized();
        let col = engine::sun::sun_color(dir);
        let sky = SkyLut::build(dir);
        Environment {
            time_of_day,
            sun: dir,
            sun_col: [col[0], col[1], col[2]],
            sky: Arc::new(sky),
            water_level,
            contours,
            shadows,
            env_time: 0.0,
            atmo,
            backdrop,
        }
    }

    /// The pretty render look: mid-morning sun (10:00), full atmosphere.
    pub fn day() -> Self {
        Self::new(10.0 * engine::sun::HOUR, true, true, None, true, ENV_DAY.clear_rgb())
    }

    /// The clean editor look: same sun for the lambert shading, but NO
    /// atmosphere — flat backdrop + unlit floor so the grid overlay can
    /// cover the whole ground (the user's "full grid, no green, no fog").
    pub fn editor(backdrop: [f32; 3]) -> Self {
        Self::new(10.0 * engine::sun::HOUR, false, false, None, false, backdrop)
    }

    /// CLI wiring: build the environment from env vars so every headless
    /// render is data-driven (no hardcoded sun):
    /// - `ROADKIT_TIME`     — hours 0..24 (default 10)
    /// - `ROADKIT_SHADOWS`  — 0/off disables the shadow map (default on)
    /// - `ROADKIT_CONTOURS` — 1/on enables the 10 m/50 m contour lines
    /// - `ROADKIT_WATER`    — sea level in meters (default: no water)
    /// - `ROADKIT_ATMO`     — 0/off = flat editor backdrop, no fog/sky
    /// - `ROADKIT_BG`       — "#rrggbb" flat backdrop (default ENV_DAY)
    pub fn from_env() -> Self {
        let mut env = Self::new(
            Self::hours_from_env() * engine::sun::HOUR,
            Self::flag_from_env("ROADKIT_SHADOWS", true),
            Self::flag_from_env("ROADKIT_CONTOURS", true),
            std::env::var("ROADKIT_WATER").ok().and_then(|v| v.parse().ok()),
            Self::flag_from_env("ROADKIT_ATMO", true),
            Self::bg_from_env(),
        );
        if !env.atmo {
            // the editor look has no sun map to lean on — keep shadows off
            // unless explicitly asked (they only show on the roads anyway)
            env.shadows = Self::flag_from_env("ROADKIT_SHADOWS", false);
        }
        env
    }

    fn hours_from_env() -> f64 {
        std::env::var("ROADKIT_TIME")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10.0)
    }

    fn flag_from_env(name: &str, default: bool) -> bool {
        std::env::var(name)
            .map(|v| {
                let v = v.to_lowercase();
                if v == "0" || v == "off" || v == "false" {
                    false
                } else if v == "1" || v == "on" || v == "true" {
                    true
                } else {
                    default
                }
            })
            .unwrap_or(default)
    }

    fn bg_from_env() -> [f32; 3] {
        std::env::var("ROADKIT_BG")
            .ok()
            .and_then(|v| {
                let v = v.trim().trim_start_matches('#');
                u32::from_str_radix(v, 16)
                    .ok()
                    .map(|n| [(n >> 16) as f32 / 255.0, ((n >> 8) & 0xff) as f32 / 255.0, (n & 0xff) as f32 / 255.0])
            })
            .unwrap_or_else(|| ENV_DAY.clear_rgb())
    }
}

/// Render the scene with the full Egregoria environment: analytic sun +
/// sky LUT + aerial-perspective fog + sun shadows + grass-textured ground
/// + optional water plane + contours, composed with the Egregoria
/// tonemap. This is the R15 environment entry used by the editor (wasm +
/// native) and the CLI.
#[allow(clippy::too_many_arguments)]
pub fn render_env_shaded(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: &Environment,
    extra: &[PropDraw],
    show_props: bool,
    shade: ShadeMode,
) -> Vec<u8> {
    // Quality = 2× SSAA: render the env path at double size, downsample
    // the linear HDR buffer, then compose once at output resolution.
    if shade == ShadeMode::Quality {
        let (hdr, depth) = render_env_buffers(
            scene, library, textures, cam, w * 2, h * 2, env, extra, show_props,
        );
        let (hdr, depth) = box_downsample_f32(&hdr, &depth, w * 2, h * 2, 2);
        return compose(
            &hdr, &depth, w, h, cam, env,
        );
    }
    let (hdr, depth) = render_env_buffers(scene, library, textures, cam, w, h, env, extra, show_props);
    compose(&hdr, &depth, w, h, cam, env)
}

/// `render_env_shaded` with the props toggle.
#[allow(clippy::too_many_arguments)]
pub fn render_env_props_shaded(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: &Environment,
    extra: &[PropDraw],
    show_props: bool,
    shade: ShadeMode,
) -> Vec<u8> {
    if show_props {
        render_env_shaded(scene, library, textures, cam, w, h, env, extra, true, shade)
    } else {
        let mut bare = scene.clone();
        bare.props.clear();
        let empty: Vec<PropDraw> = Vec::new();
        let ex: &[PropDraw] = if extra.is_empty() { &empty } else { extra };
        render_env_shaded(&bare, library, textures, cam, w, h, env, ex, false, shade)
    }
}

/// `render_env_shaded` that ALSO returns the raster's view-depth buffer
/// (w×h, view-space z: negative in front of the camera, larger = closer,
/// NEG_INFINITY = nothing drawn there). The editor's screen-space ground
/// grid depth-tests against it — the VectorCraft layering law: the grid is
/// the BOTTOM layer and the roads/bridges are seen on top of it.
#[allow(clippy::too_many_arguments)]
pub fn render_env_shaded_depth(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: &Environment,
    extra: &[PropDraw],
    show_props: bool,
    shade: ShadeMode,
) -> (Vec<u8>, Vec<f32>) {
    if shade == ShadeMode::Quality {
        let (hdr, depth) = render_env_buffers(
            scene, library, textures, cam, w * 2, h * 2, env, extra, show_props,
        );
        let (hdr, depth) = box_downsample_f32(&hdr, &depth, w * 2, h * 2, 2);
        let rgba = compose(&hdr, &depth, w, h, cam, env);
        return (rgba, depth);
    }
    let (hdr, depth) = render_env_buffers(scene, library, textures, cam, w, h, env, extra, show_props);
    let rgba = compose(&hdr, &depth, w, h, cam, env);
    (rgba, depth)
}

/// `render_env_shaded_depth` with the props toggle (the app's entry).
#[allow(clippy::too_many_arguments)]
pub fn render_env_props_shaded_depth(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: &Environment,
    extra: &[PropDraw],
    show_props: bool,
    shade: ShadeMode,
) -> (Vec<u8>, Vec<f32>) {
    if show_props {
        render_env_shaded_depth(scene, library, textures, cam, w, h, env, extra, true, shade)
    } else {
        let mut bare = scene.clone();
        bare.props.clear();
        let empty: Vec<PropDraw> = Vec::new();
        let ex: &[PropDraw] = if extra.is_empty() { &empty } else { extra };
        render_env_shaded_depth(&bare, library, textures, cam, w, h, env, ex, false, shade)
    }
}

/// Raster the environment path into (linear HDR rgb3, view-depth).
#[allow(clippy::too_many_arguments)]
fn render_env_buffers(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: &Environment,
    extra: &[PropDraw],
    show_props: bool,
) -> (Vec<f32>, Vec<f32>) {
    // single-cascade sun shadow map (objects cast; ground receives)
    let shadow = if env.shadows && env.sun.y > 0.02 {
        Some(build_shadow_map(scene, library, cam, env))
    } else {
        None
    };
    // water plane (drawn last, z-tested against the terrain behind it)
    let water = env
        .water_level
        .filter(|_| env.atmo)
        .map(|level| build_water_mesh(scene, level));

    // EDITOR look (atmo off): no aerial-perspective fog, no grass ground —
    // an unlit flat floor quad that matches the backdrop exactly, so the
    // viewport grid overlay can cover the whole ground ("full grid, no
    // green, no whitish cloud").
    let fog_lut: Option<Arc<FogLut>> = if env.atmo {
        Some(Arc::new(FogLut::build(cam.view_dir(), env.sun)))
    } else {
        None
    };
    let floor = if env.atmo {
        None
    } else {
        Some(build_editor_floor(env, scene, cam))
    };
    let props: &[PropDraw] = if show_props { extra } else { &[] };
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16)
        .min(h / 120.max(1));
    let run = |frame: &mut Frame| {
        frame.env_mode = true;
        frame.light = env.sun;
        frame.sun_col = env.sun_col;
        frame.eye = cam.eye();
        frame.env_time = env.env_time;
        frame.contours = env.contours;
        frame.shadow = shadow.clone();
        frame.fog_lut = fog_lut.clone();
        frame.sky = Some(env.sky.clone());
        draw_all(
            frame,
            scene,
            library,
            textures,
            cam,
            env_sky_bg(),
            props,
            true,
            water.as_ref(),
            floor.as_ref(),
        );
    };
    if threads <= 1 {
        let mut frame = Frame::new(w, h, env_sky_bg());
        run(&mut frame);
        return (frame.hdr, frame.depth);
    }
    let band = h.div_ceil(threads);
    let mut bands: Vec<(usize, usize)> = Vec::new();
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
                    let mut frame = Frame::band(w, bh, env_sky_bg(), yb, h);
                    run(&mut frame);
                    frame
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|hnd| hnd.join().expect("render env band thread"))
            .collect()
    });
    // stitch hdr + depth bands
    let mut hdr = vec![0f32; w * h * 3];
    let mut depth = vec![f32::NEG_INFINITY; w * h];
    for (row, (yb, bh)) in bands.iter().enumerate() {
        let src = &results[row];
        let off = yb * w * 3;
        hdr[off..off + bh * w * 3].copy_from_slice(&src.hdr[..bh * w * 3]);
        let off = yb * w;
        depth[off..off + bh * w].copy_from_slice(&src.depth[..bh * w]);
    }
    (hdr, depth)
}

/// Neutral clear for the env path (the sky LUT overwrites sky pixels by
/// depth, so the u8 backdrop is irrelevant here).
fn env_sky_bg() -> [u8; 4] {
    [0, 0, 0, 255]
}

/// Final compose for the env path (sky + tonemap; see raster::compose_env).
#[allow(clippy::too_many_arguments)]
fn compose(
    hdr: &[f32],
    depth: &[f32],
    w: usize,
    h: usize,
    cam: &gdmath::OrbitCam,
    env: &Environment,
) -> Vec<u8> {
    let view = cam.view();
    let p = EnvCompose {
        eye: cam.eye(),
        view: &view,
        fov: cam.fov,
        w,
        h,
        sun: env.sun,
        sky: &env.sky,
        flat_bg: if env.atmo {
            None
        } else {
            Some(editor_backdrop_linear(env))
        },
    };
    let mut hdr = hdr.to_vec();
    crate::raster::compose_env(&mut hdr, depth, &p)
}

/// The editor floor / sky backdrop as a LINEAR pre-exposure color whose
/// tonemapped + gamma-encoded output is EXACTLY `env.backdrop` (sRGB):
/// inverts autoexposure(sun.y) and tonemap y = 1.83c/(1.24c+1.68), then the
/// gamma-2.0 encode (out = sqrt(c)). Floor and sky carry the same value, so
/// both encode to the identical paper color.
fn editor_backdrop_linear(env: &Environment) -> [f32; 3] {
    let auto = engine::atmosphere::autoexposure(env.sun.y);
    let inv = |y: f32| -> f32 {
        // y = encoded sRGB target (0..1, post gamma-2). Reconstruct:
        // pre-gamma c_f = y*y; c_post_tm = c_f / auto... order: out=sqrt(tm)
        // so tm = y²; tm(c) = 1.83c/(1.24c+1.68) → c = 1.68·tm/(1.83−1.24·tm)
        let tm = (y * y).clamp(0.0, 0.999);
        let c = 1.68 * tm / (1.83 - 1.24 * tm).max(1e-6);
        (c / auto).clamp(0.0, 64.0)
    };
    [inv(env.backdrop[0]), inv(env.backdrop[1]), inv(env.backdrop[2])]
}

/// The unlit editor floor: one big quad at y = −0.02 (just under the road
/// surfaces), side 2·extent, albedo = the backdrop color. Drawn ONLY when
/// the atmosphere is off (see render_env_buffers); `Frame::flat_floor`
/// forces the fragment output to the raw albedo so it encodes to exactly
/// the backdrop color and melts into the sky with no visible horizon.
pub fn editor_extent(scene: &Scene, cam: &gdmath::OrbitCam) -> f32 {
    (scene.radius_m * 3.0).max(500.0).max(cam.dist * 3.0)
}

fn build_editor_floor(env: &Environment, scene: &Scene, cam: &gdmath::OrbitCam) -> ScenePart {
    let e = editor_extent(scene, cam);
    ScenePart {
        positions: vec![
            Vec3::new(-e, -0.02, -e),
            Vec3::new(e, -0.02, -e),
            Vec3::new(-e, -0.02, e),
            Vec3::new(e, -0.02, e),
        ],
        // two CCW triangles seen from above (+y)
        indices: vec![0, 2, 1, 1, 2, 3],
        color: [env.backdrop[0], env.backdrop[1], env.backdrop[2], 1.0],
        vcolors: vec![],
        uv: vec![],
        tex: -1,
    }
}

/// Single-cascade sun shadow map: an ortho fit around the camera target,
/// depth-only raster of all road/junction/prop geometry (the ground does
/// NOT cast — Egregoria's terrain doesn't either; it only receives).
fn build_shadow_map(
    scene: &Scene,
    library: &AssetLibrary,
    cam: &gdmath::OrbitCam,
    env: &Environment,
) -> Arc<ShadowData> {
    let size = if cfg!(target_arch = "wasm32") { 512 } else { 1024 };
    let ext = (scene.radius_m * 1.5).clamp(80.0, 900.0);
    let sun = env.sun;
    let up = if sun.y > 0.985 {
        Vec3::new(0.0, 0.0, -1.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let eye = Vec3::new(
        cam.target.x + sun.x * ext * 3.0,
        cam.target.y + sun.y * ext * 3.0,
        cam.target.z + sun.z * ext * 3.0,
    );
    let view = Mat4::look_at(eye, cam.target, up);
    let near = 1.0f32;
    let far = ext * 6.0;
    let proj = Mat4::ortho(-ext, ext, -ext, ext, near, far);
    let mut frame = Frame::new(size, size, [0, 0, 0, 255]);
    frame.depth_only = true;
    for part in &scene.parts {
        if part.positions.is_empty() {
            continue;
        }
        frame.draw_mesh(
            &part.positions,
            &part.indices,
            None,
            None,
            None,
            &Mat4::IDENTITY,
            &view,
            &proj,
            eye,
            sun,
            part.color,
        );
    }
    for pd in &scene.props {
        let Some(entry) = library.entries.get(pd.asset) else {
            continue;
        };
        for prim in &entry.mesh.prims {
            frame.draw_mesh(
                &prim.positions,
                &prim.indices,
                None,
                None,
                None,
                &pd.model,
                &view,
                &proj,
                eye,
                sun,
                prim.color,
            );
        }
    }
    Arc::new(ShadowData {
        size,
        depth: frame.depth,
        view,
        ext,
        near,
        far,
    })
}

/// The water plane: a flat grid at the sea level spanning the ground reach.
/// Shading is fully procedural (Gerstner + TEX_WAVY + sky reflection), so
/// the vertex grid only exists for clipping/culling granularity.
fn build_water_mesh(scene: &Scene, level: f32) -> ScenePart {
    let ext = (scene.ground_extent.max(scene.radius_m) * 1.35).max(120.0);
    let n = 24usize;
    let mut p = ScenePart {
        positions: vec![],
        indices: vec![],
        color: [1.0, 1.0, 1.0, 1.0],
        vcolors: vec![],
        uv: vec![],
        tex: crate::textures::TEX_WAVY,
    };
    for j in 0..=n {
        for i in 0..=n {
            let x = -ext + 2.0 * ext * i as f32 / n as f32;
            let z = -ext + 2.0 * ext * j as f32 / n as f32;
            p.positions.push(Vec3::new(x, level, z));
        }
    }
    let stride = (n + 1) as u32;
    for j in 0..n as u32 {
        for i in 0..n as u32 {
            let a = j * stride + i;
            p.indices
                .extend_from_slice(&[a, a + stride, a + 1, a + 1, a + stride, a + stride + 1]);
        }
    }
    p
}

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

/// Flat engine-terrain ground (terrain connector OFF): a uniform GRID at
/// y = -0.04 (R15 — the old 3-ring 96-segment fan produced wedge-shaped
/// shading/mip stripes radiating from the origin; a grid has none). Same
/// grass-texture + tint + rim treatment as the terrain heightfield.
fn build_flat_grass(
    ground: &mut Vec<ScenePart>,
    radius: f32,
    tris: &mut usize,
    horizon: [f32; 3],
) {
    let inv = 1.0 / up_shade();
    let rim = [horizon[0] * inv, horizon[1] * inv, horizon[2] * inv];
    let extent = radius * 1.6;
    let spacing = ((2.0 * extent) / 95.0).max(8.0);
    let n = (((2.0 * extent) / spacing).floor() as usize).clamp(2, 96);
    let step = (2.0 * extent) / n as f32;
    let stride = n + 1;
    let mut part = ScenePart {
        positions: vec![],
        indices: vec![],
        color: [1.0, 1.0, 1.0, 1.0],
        vcolors: vec![],
        uv: vec![],
        tex: crate::textures::TEX_TERRAIN_GRASS,
    };
    let vc = |x: f32, z: f32| -> [f32; 4] {
        let t = (x.abs() / extent).max(z.abs() / extent);
        // melt the outer 55% of the disc into the haze so the grid edge
        // never shows a hard line against the sky
        let f = smoothstep(0.45, 1.0, t);
        grass_tint([
            GRASS_BASE[0] + (rim[0] - GRASS_BASE[0]) * f,
            GRASS_BASE[1] + (rim[1] - GRASS_BASE[1]) * f,
            GRASS_BASE[2] + (rim[2] - GRASS_BASE[2]) * f,
        ])
    };
    for j in 0..n {
        for i in 0..n {
            let x0 = -extent + i as f32 * step;
            let x1 = x0 + step;
            let z0 = -extent + j as f32 * step;
            let z1 = z0 + step;
            let base = part.positions.len() as u32;
            part.positions.extend_from_slice(&[
                Vec3::new(x0, -0.04, z0),
                Vec3::new(x1, -0.04, z0),
                Vec3::new(x0, -0.04, z1),
                Vec3::new(x1, -0.04, z1),
            ]);
            part.uv.extend_from_slice(&[
                [x0 / 200.0, z0 / 200.0],
                [x1 / 200.0, z0 / 200.0],
                [x0 / 200.0, z1 / 200.0],
                [x1 / 200.0, z1 / 200.0],
            ]);
            part.vcolors.extend_from_slice(&[
                vc(x0, z0),
                vc(x1, z0),
                vc(x0, z1),
                vc(x1, z1),
            ]);
            part.indices
                .extend_from_slice(&[base, base + 1, base + 2, base + 2, base + 1, base + 3]);
        }
    }
    *tris += part.indices.len() / 3;
    ground.push(part);
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
    render_scene_shaded(scene, library, textures, cam, w, h, bg, extra, ShadeMode::Solid)
}

/// `render_scene` with a Blender-style viewport shading mode.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_shaded(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    bg: [u8; 4],
    extra: &[PropDraw],
    shade: ShadeMode,
) -> Vec<u8> {
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(1)
        .clamp(1, 16)
        .min(h / 120.max(1));
    if threads <= 1 {
        let mut frame = Frame::new(w, h, bg);
        frame.shade = shade;
        draw_all(
            &mut frame, scene, library, textures, cam, bg, extra, false, None, None,
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
                    frame.shade = shade;
                    draw_all(&mut frame, scene, library, textures, cam, bg, extra, false, None, None);
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
    render_scene_env_shaded(scene, library, textures, cam, w, h, env, extra, ShadeMode::Solid)
}

/// `render_scene_env` with a viewport shading mode.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_env_shaded(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: Env,
    extra: &[PropDraw],
    shade: ShadeMode,
) -> Vec<u8> {
    // Quality = 2× SSAA supersampled Solid
    if shade == ShadeMode::Quality {
        let big = render_scene_shaded(
            scene, library, textures, cam, w * 2, h * 2, env.clear, extra, ShadeMode::Solid,
        );
        let mut rgba = crate::raster::box_downsample(&big, w * 2, h * 2, 2);
        paint_sky(&mut rgba, w, h, cam.pitch, env);
        return rgba;
    }
    let mut rgba = render_scene_shaded(scene, library, textures, cam, w, h, env.clear, extra, shade);
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
    render_scene_props_env_shaded(scene, library, textures, cam, w, h, env, extra, show_props, ShadeMode::Solid)
}

/// `render_scene_props_env` with a viewport shading mode.
#[allow(clippy::too_many_arguments)]
pub fn render_scene_props_env_shaded(
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    env: Env,
    extra: &[PropDraw],
    show_props: bool,
    shade: ShadeMode,
) -> Vec<u8> {
    if show_props {
        render_scene_env_shaded(scene, library, textures, cam, w, h, env, extra, shade)
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
/// R15: `env_mode` switches the Egregoria shading (analytic sun, shadows,
/// atmosphere fog, grass ground) and draws the optional water plane last.
#[allow(clippy::too_many_arguments)]
fn draw_all(
    frame: &mut Frame,
    scene: &Scene,
    library: &AssetLibrary,
    textures: &TextureLibrary,
    cam: &gdmath::OrbitCam,
    bg: [u8; 4],
    extra: &[PropDraw],
    env_mode: bool,
    water: Option<&ScenePart>,
    // EDITOR look: when Some, the grass/terrain ground is SKIPPED and this
    // unlit backdrop-colored quad is drawn instead (frame.flat_floor).
    floor: Option<&ScenePart>,
) {
    if env_mode {
        // the atmosphere fog LUT replaces the legacy linear blend
        frame.fog_on = false;
    } else {
        // distance fog measured from the CAMERA: content near the orbit target
        // stays clear, the far roads fade into the background
        frame.set_fog(cam.dist + scene.radius_m * 0.55, cam.dist + scene.radius_m * 3.4, bg);
    }
    frame.depth_range = (
        (cam.dist - scene.radius_m * 0.3).max(2.0),
        cam.dist + scene.radius_m * 3.4,
    );
    // R16 FIX: the aspect must come from FULL_h — `frame.h` is the BAND
    // height when rendering multithreaded (2 bands of 414px on this machine
    // doubled the aspect and squeezed the whole render to ~50% width about
    // the viewport centre, so the meshes no longer lined up with the
    // screen-space overlays: the user's "points and render not aligned").
    let view = cam.view();
    let proj = cam.proj(frame.w as f32 / frame.full_h.max(1) as f32);
    let light = if env_mode {
        frame.light
    } else {
        Vec3::new(0.4, 0.85, 0.3).normalized()
    };
    let eye = cam.eye();

    // R12 perf: frustum culling (Gribb–Hartmann planes from the view-proj).
    // The CPU raster used to transform EVERY vertex of the whole corridor
    // each frame — on wasm a Kijazi closeup sat at ~125 ms/frame. Parts and
    // props fully outside the frustum are skipped before draw_mesh.
    let vp = proj.mul(&view);
    let row = |i: usize| (vp.m[i * 4], vp.m[i * 4 + 1], vp.m[i * 4 + 2], vp.m[i * 4 + 3]);
    let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
    let mut planes = [
        (r3.0 + r0.0, r3.1 + r0.1, r3.2 + r0.2, r3.3 + r0.3), // left
        (r3.0 - r0.0, r3.1 - r0.1, r3.2 - r0.2, r3.3 - r0.3), // right
        (r3.0 + r1.0, r3.1 + r1.1, r3.2 + r1.2, r3.3 + r1.3), // bottom
        (r3.0 - r1.0, r3.1 - r1.1, r3.2 - r1.2, r3.3 - r1.3), // top
        (r3.0 + r2.0, r3.1 + r2.1, r3.2 + r2.2, r3.3 + r2.3), // near
        (r3.0 - r2.0, r3.1 - r2.1, r3.2 - r2.2, r3.3 - r2.3), // far
    ];
    for pl in &mut planes {
        let len = (pl.0 * pl.0 + pl.1 * pl.1 + pl.2 * pl.2).sqrt();
        if len > 1e-6 {
            pl.0 /= len;
            pl.1 /= len;
            pl.2 /= len;
            pl.3 /= len;
        }
    }
    // sphere (centroid + radius) vs the 6 planes — outside any one ⇒ reject
    let outside = |cx: f32, cy: f32, cz: f32, r: f32| {
        planes.iter().any(|pl| pl.0 * cx + pl.1 * cy + pl.2 * cz + pl.3 < -r)
    };

    frame.ground_pass = env_mode;
    let ground_iter: Vec<&ScenePart> = if floor.is_some() {
        Vec::new()
    } else {
        scene.ground.iter().collect()
    };
    if let Some(fl) = floor {
        // the editor floor: unlit, exactly the backdrop color (see
        // Frame::flat_floor in the env fragment)
        frame.flat_floor = true;
        frame.draw_mesh(
            &fl.positions,
            &fl.indices,
            None,
            None,
            None,
            &Mat4::IDENTITY,
            &view,
            &proj,
            eye,
            light,
            fl.color,
        );
        frame.flat_floor = false;
    }
    for part in ground_iter.into_iter().chain(scene.parts.iter()) {
        // coarse bounds from a strided sample (the parts are per-strip, so a
        // 1-in-12 sample bounds the sphere tightly enough to cull)
        let n = part.positions.len();
        if n == 0 {
            continue;
        }
        // ~24 samples bound the sphere tightly enough to cull
        let step = (n / 24).max(1);
        let mut sx = 0.0f32;
        let mut sy = 0.0f32;
        let mut sz = 0.0f32;
        let mut cnt = 0usize;
        let mut i = 0;
        while i < n {
            sx += part.positions[i].x;
            sy += part.positions[i].y;
            sz += part.positions[i].z;
            cnt += 1;
            i += step;
        }
        let (cx, cy, cz) = (sx / cnt as f32, sy / cnt as f32, sz / cnt as f32);
        let mut r2 = 0.0f32;
        let mut j = 0;
        while j < n {
            let p = &part.positions[j];
            let dx = p.x - cx;
            let dy = p.y - cy;
            let dz = p.z - cz;
            r2 = r2.max(dx * dx + dy * dy + dz * dz);
            j += step;
        }
        // sampled radius can undershoot — pad 25 %
        let r = r2.sqrt() * 1.25;
        if outside(cx, cy, cz, r) {
            continue;
        }
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
    frame.ground_pass = false;
    for pd in scene.props.iter().chain(extra.iter()) {
        let Some(entry) = library.entries.get(pd.asset) else {
            continue;
        };
        // prop bounds: translation + a generous 12 m world radius (masts,
        // buses and buildings all fit); a fully off-frustum prop is skipped
        let (px, py, pz) = (pd.model.m[3], pd.model.m[7], pd.model.m[11]);
        if outside(px, py, pz, 12.0) {
            continue;
        }
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
    // R15: the water plane last (z-tested against the terrain behind it —
    // the shore fade reads the depth buffer BEFORE the water writes it)
    if let Some(w) = water {
        frame.water = true;
        let tex = textures.get(w.tex);
        let uvs = if tex.is_some() { Some(&w.uv[..]) } else { None };
        frame.draw_mesh(
            &w.positions,
            &w.indices,
            None,
            uvs,
            tex,
            &Mat4::IDENTITY,
            &view,
            &proj,
            eye,
            light,
            w.color,
        );
        frame.water = false;
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

        // terrain enabled grows the ground (heightfield) and rides the
        // grass albedo texture (Egregoria ground) — R15: BOTH grounds are
        // the uniform grass grid now (flat 1.6r reach vs heightfield 1.2r)
        let mut terrain = roads::terrain::TerrainState::default();
        terrain.enabled = true;
        terrain.amplitude = 6.0;
        let flat = build_scene_full(&curves, &[], &[], &[], &lib, false, false, None);
        let hilly = build_scene_full(&curves, &[], &[], &[], &lib, false, false, Some(&terrain));
        let flat_ground: usize = flat.ground.iter().map(|g| g.positions.len()).sum();
        let hilly_ground: usize = hilly.ground.iter().map(|g| g.positions.len()).sum();
        assert!(
            flat_ground > 0 && hilly_ground > 0,
            "both grounds are gridded ({} / {})",
            flat_ground,
            hilly_ground
        );
        assert!(
            hilly.ground.iter().all(|g| g.tex == crate::textures::TEX_TERRAIN_GRASS)
                && flat.ground.iter().all(|g| g.tex == crate::textures::TEX_TERRAIN_GRASS),
            "ground carries the grass albedo (Egregoria ground)"
        );
        assert!(
            hilly.ground_extent < flat.ground_extent,
            "terrain heightfield reach (1.2r) < flat grid reach (1.6r)"
        );
        assert!(
            hilly.sampler.is_some() && flat.sampler.is_none(),
            "sampler rides along only with the terrain connector"
        );
        // the classic wrapper keeps the flat behavior (no terrain, no fins)
        let flat_grid = build_scene_full(&curves, &[], &[], &[], &lib, true, false, None);
        let wrapped = build_scene(&curves, &[], &[], &lib, true);
        assert_eq!(wrapped.ground.len(), flat_grid.ground.len());
    }
}
