//! Automatic junction detection + geometry — the godot-road-generator
//! "auto join" behavior, generalized.
//!
//! Three detection channels (mirroring GRG's editor joins):
//! 1. ENDPOINT CLUSTERS — curve ends within `snap_m` of each other join up
//!    (GRG's connection-tool snap, automatic).
//! 2. CROSSINGS — two curves whose centerlines intersect get an n-gon
//!    intersection patch (GRG's IntersectionNGon, detected automatically
//!    instead of requiring the editor action).
//! 3. T-JUNCTIONS — a curve ending on the BODY of another curve.
//!
//! Classification: >= 3 legs + auto_roundabout -> roundabout ring
//! (Blender-style GLB prefab ring in GRG; procedural ring here), else an
//! n-gon crossing patch whose outline is built exactly like GRG's fan:
//! each leg contributes two corners at the stop line, connected in
//! angular order.

use crate::model::RoadCurve;
use crate::model::RoadPoint;
use crate::prefab::PrefabPlacement;
use crate::surface;
use gdmath::vec2::Vec2;

pub const DEFAULT_SNAP_M: f64 = 8.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JunctionKind {
    Crossing,
    Roundabout,
}

impl JunctionKind {
    pub fn label(&self) -> &'static str {
        match self {
            JunctionKind::Crossing => "crossing",
            JunctionKind::Roundabout => "roundabout",
        }
    }
}

/// One leg of a junction: a road touching the junction point.
#[derive(Clone, Debug)]
pub struct JunctionLeg {
    /// curve index into the scene
    pub curve: usize,
    /// unit vector FROM the junction TOWARDS the leg (outward)
    pub out_dir: (f64, f64),
    pub half_width: f64,
    /// true when this leg is a curve endpoint (else a through-crossing leg)
    pub is_end: bool,
    /// when is_end: 0 = the curve's first point side, 1 = its last point side
    pub end: u8,
}

impl JunctionLeg {
    fn key(&self) -> (usize, u8, i64, i64) {
        (
            self.curve,
            if self.is_end { self.end } else { 2 },
            (self.out_dir.0 * 16.0).round() as i64,
            (self.out_dir.1 * 16.0).round() as i64,
        )
    }
}

#[derive(Clone, Debug)]
pub struct Junction {
    pub x: f64,
    pub z: f64,
    pub kind: JunctionKind,
    pub legs: Vec<JunctionLeg>,
    /// asphalt outline in meters (plan view x, z). n-gon for crossings,
    /// 24-gon circle for roundabouts.
    pub outline: Vec<(f64, f64)>,
    /// roundabout only: (center x, center z, island radius)
    pub island: Option<(f64, f64, f64)>,
    /// roundabout outer radius (ring edge)
    pub ring_outer: f64,
    /// WFC-collapsed GLB prefab placement (None = procedural patch)
    pub prefab: Option<PrefabPlacement>,
}

#[derive(Clone, Debug)]
pub struct JunctionSettings {
    /// weld/join distance (meters) — GRG's snap threshold
    pub snap_m: f64,
    /// classify 3+ leg junctions as roundabouts
    pub auto_roundabout: bool,
    /// roundabout island radius (meters)
    pub island_radius: f64,
    /// master switch — when off, nothing is detected
    pub enabled: bool,
    /// collapse detected junctions to the hand-modeled GLB prefab tiles
    /// (WFC) instead of the procedural patches when the geometry fits
    pub use_prefabs: bool,
    /// include the fuzzy tiles (splitter / on-ramp / off-ramp)
    pub prefabs_manual: bool,
}

impl Default for JunctionSettings {
    fn default() -> Self {
        Self {
            snap_m: DEFAULT_SNAP_M,
            auto_roundabout: true,
            island_radius: 5.0,
            enabled: true,
            use_prefabs: true,
            prefabs_manual: false,
        }
    }
}

/* ------------------------------------------------------------------ */
/* Detection                                                           */
/* ------------------------------------------------------------------ */

struct Candidate {
    x: f64,
    z: f64,
    legs: Vec<JunctionLeg>,
}

fn leg_perp(d: (f64, f64)) -> (f64, f64) {
    // right of travel in (x east, z south) screen space
    (-d.1, d.0)
}

/// Flattened polyline + per-vertex half width for junction math.
struct Flat {
    pts: Vec<Vec2>,
    yaw: Vec<f64>,
    /// per-sample half paved width (lerped between the RoadPoints that
    /// bound each segment — per-point cross-section, GRG style)
    halfs: Vec<f64>,
    /// per-sample road surface elevation (meters) — grade separation
    elevs: Vec<f64>,
    /// OSM-style layer (max over points) — 0 = ground, 1 = flyover deck
    layer: i8,
}

fn flatten(c: &RoadCurve, sample_m: f64) -> Flat {
    // flatten per segment so each sample knows its t — this is what makes
    // junction legs pick up the width AT the junction, not at point 0
    let n_points = c.points.len();
    let layer = c.max_layer();
    let seg_count = if c.closed { n_points } else { n_points - 1 };
    let mut pts: Vec<Vec2> = Vec::new();
    let mut halfs: Vec<f64> = Vec::new();
    let mut elevs: Vec<f64> = Vec::new();
    for k in 0..seg_count {
        let a = &c.points[k];
        let b = &c.points[(k + 1) % n_points];
        let seg = crate::curve::segment_cubic(a, b);
        let (sp, sc, seg_len) = crate::curve::flatten_cubic(&seg, sample_m.max(1.0));
        let hw_a = c.paved_width_at(k) / 2.0;
        let hw_b = c.paved_width_at((k + 1) % n_points) / 2.0;
        for (j, p) in sp.iter().enumerate() {
            if j == 0 && !pts.is_empty() {
                continue;
            }
            let t = if seg_len > 1e-9 {
                (sc[j] / seg_len).clamp(0.0, 1.0)
            } else {
                0.0
            };
            pts.push(*p);
            halfs.push(hw_a + (hw_b - hw_a) * t);
            elevs.push(a.elev + (b.elev - a.elev) * t);
        }
    }
    let mut yaw = Vec::with_capacity(pts.len());
    for i in 0..pts.len() {
        let a = pts[i.saturating_sub(1)];
        let b = pts[(i + 1).min(pts.len() - 1)];
        yaw.push(atan2f(b.y - a.y, b.x - a.x));
    }
    Flat { pts, yaw, halfs, elevs, layer }
}

#[allow(clippy::many_single_char_names)]
fn seg_intersect(p: Vec2, r: Vec2, q: Vec2, s: Vec2) -> Option<Vec2> {
    let rx = r.x;
    let ry = r.y;
    let sx = s.x;
    let sy = s.y;
    let denom = rx * sy - ry * sx;
    if denom.abs() < 1e-9 {
        return None;
    }
    let qpx = q.x - p.x;
    let qpy = q.y - p.y;
    let t = (qpx * sy - qpy * sx) / denom;
    let u = (qpx * ry - qpy * rx) / denom;
    if t < 0.0 || t > 1.0 || u < 0.0 || u > 1.0 {
        return None;
    }
    Some(Vec2::new(p.x + t * rx, p.y + t * ry))
}

fn dist_point_seg(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let abx = b.x - a.x;
    let abz = b.y - a.y;
    let len2 = abx * abx + abz * abz;
    if len2 < 1e-9 {
        return p.dist(a);
    }
    let t = (((p.x - a.x) * abx + (p.y - a.y) * abz) / len2).clamp(0.0, 1.0);
    let cx = a.x + t * abx;
    let cy = a.y + t * abz;
    ((p.x - cx).powi(2) + (p.y - cy).powi(2)).sqrt()
}

/// Detect every junction in the scene (endpoints joins, crossings, T-joins).
pub fn detect(curves: &[RoadCurve], s: &JunctionSettings) -> Vec<Junction> {
    let mut out = Vec::new();
    if !s.enabled || curves.is_empty() {
        return out;
    }

    let flats: Vec<Flat> = curves.iter().map(|c| flatten(c, 8.0)).collect();
    let mut candidates: Vec<Candidate> = Vec::new();

    // ---- 1. endpoint clusters -------------------------------------
    #[derive(Clone)]
    struct EndRef {
        curve: usize,
        end: u8,
        x: f64,
        z: f64,
        dir_out: (f64, f64),
        half: f64,
    }
    let mut ends: Vec<EndRef> = Vec::new();
    for (i, c) in curves.iter().enumerate() {
        if c.closed || c.points.is_empty() {
            continue;
        }
        // first point side: outward dir = -travel dir
        let d0 = c.points[0].dir();
        ends.push(EndRef {
            curve: i,
            end: 0,
            x: c.points[0].x,
            z: c.points[0].z,
            dir_out: (-d0.0, -d0.1),
            half: c.paved_width_at(0) / 2.0,
        });
        let last = c.points.len() - 1;
        if last == 0 {
            continue;
        }
        let dl = c.points[last].dir();
        ends.push(EndRef {
            curve: i,
            end: 1,
            x: c.points[last].x,
            z: c.points[last].z,
            dir_out: (dl.0, dl.1),
            half: c.paved_width_at(last) / 2.0,
        });
    }
    // greedy cluster by distance (endpoints count is small)
    let mut used = vec![false; ends.len()];
    for i in 0..ends.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];
        used[i] = true;
        for j in (i + 1)..ends.len() {
            if used[j] {
                continue;
            }
            let d = ((ends[j].x - ends[i].x).powi(2) + (ends[j].z - ends[i].z).powi(2)).sqrt();
            if d <= s.snap_m {
                group.push(j);
                used[j] = true;
            }
        }
        let n = group.len() as f64;
        let (mut mx, mut mz) = (0.0, 0.0);
        for &g in &group {
            mx += ends[g].x;
            mz += ends[g].z;
        }
        mx /= n;
        mz /= n;
        let legs: Vec<JunctionLeg> = group
            .iter()
            .map(|&g| JunctionLeg {
                curve: ends[g].curve,
                out_dir: ends[g].dir_out,
                half_width: ends[g].half,
                is_end: true,
                end: ends[g].end,
            })
            .collect();
        candidates.push(Candidate { x: mx, z: mz, legs });
    }

    // ---- 2. pairwise curve crossings ------------------------------
    for i in 0..curves.len() {
        if curves[i].points.len() < 2 {
            continue;
        }
        for j in (i + 1)..curves.len() {
            if curves[j].points.len() < 2 {
                continue;
            }
            let (a, b) = (&flats[i], &flats[j]);
            'segs: for si in 0..a.pts.len() - 1 {
                for sj in 0..b.pts.len() - 1 {
                    let p = a.pts[si];
                    let r = a.pts[si + 1] - p;
                    let q = b.pts[sj];
                    let t = b.pts[sj + 1] - q;
                    // cheap bbox reject
                    if (p.x - q.x).abs() > r.x.abs() + t.x.abs() + 1.0
                        && (p.y - q.y).abs() > r.y.abs() + t.y.abs() + 1.0
                    {
                        continue;
                    }
                    let Some(hit) = seg_intersect(p, r, q, t) else {
                        continue;
                    };
                    // skip hits at curve endpoints — those are cluster joins
                    let endish = |f: &Flat, k: usize| {
                        k == 0 || k == f.pts.len() - 1
                    };
                    if endish(a, si) || endish(a, si + 1) || endish(b, sj) || endish(b, sj + 1) {
                        continue;
                    }
                    let d0 = (a.yaw[si].cos(), a.yaw[si].sin());
                    let d1 = (b.yaw[sj].cos(), b.yaw[sj].sin());
                    let ha = a.halfs[(si + 1).min(a.halfs.len() - 1)];
                    let hb = b.halfs[(sj + 1).min(b.halfs.len() - 1)];
                    // GRADE SEPARATION (JICA §5.2.5 / §5.3.5 flyover
                    // support): two curves on different OSM layers, or on
                    // the same layer but physically separated by > 2.5 m
                    // of elevation at the crossing, do NOT form an
                    // at-grade junction — the flyover passes over.
                    let ea = a.elevs[(si + 1).min(a.elevs.len() - 1)];
                    let eb = b.elevs[(sj + 1).min(b.elevs.len() - 1)];
                    if a.layer != b.layer || (ea - eb).abs() > 2.5 {
                        continue;
                    }
                    candidates.push(Candidate {
                        x: hit.x,
                        z: hit.y,
                        legs: vec![
                            JunctionLeg { curve: i, out_dir: (-d0.0, -d0.1), half_width: ha, is_end: false, end: 0 },
                            JunctionLeg { curve: i, out_dir: d0, half_width: ha, is_end: false, end: 0 },
                            JunctionLeg { curve: j, out_dir: (-d1.0, -d1.1), half_width: hb, is_end: false, end: 0 },
                            JunctionLeg { curve: j, out_dir: d1, half_width: hb, is_end: false, end: 0 },
                        ],
                    });
                    break 'segs; // one crossing per curve pair
                }
            }
        }
    }

    // ---- 3. T-junctions: a single endpoint on another curve's body --
    for e in &ends {
        // only orphan ends (not already clustered with another end)
        let clustered = candidates.iter().any(|c| {
            (c.x - e.x).hypot(c.z - e.z) <= s.snap_m && c.legs.len() > 1
        });
        if clustered {
            continue;
        }
        let p = Vec2::new(e.x, e.z);
        let mut best: Option<(f64, usize, usize, Vec2)> = None;
        for (k, f) in flats.iter().enumerate() {
            if k == e.curve || f.pts.len() < 2 {
                continue;
            }
            // skip if the other curve ends right here (that's a cluster join)
            for (m, seg_start) in f.pts.iter().enumerate() {
                let seg_end = f.pts[(m + 1).min(f.pts.len() - 1)];
                let d = dist_point_seg(p, *seg_start, seg_end);
                if best.map(|(bd, _, _, _)| d < bd).unwrap_or(true) {
                    best = Some((d, k, m, *seg_start));
                }
            }
        }
        let Some((d, k, m, _)) = best else { continue };
        // width of the through road AT the T-point (per-sample cross-section)
        let other_hw = flats[k].halfs[m.min(flats[k].halfs.len() - 1)];
        if d > other_hw + 1.5 {
            continue;
        }
        // grade separation: an endpoint sitting under/over a flyover deck
        // (different layer, or deck > 2.5 m above the stem) does NOT join
        let stem_flat = &flats[e.curve];
        let stem_elev = if e.end == 1 {
            stem_flat.elevs.last().copied().unwrap_or(0.0)
        } else {
            stem_flat.elevs.first().copied().unwrap_or(0.0)
        };
        let body_elev = flats[k].elevs[m.min(flats[k].elevs.len() - 1)];
        if flats[e.curve].layer != flats[k].layer || (stem_elev - body_elev).abs() > 2.5 {
            continue;
        }
        let dk = (flats[k].yaw[m].cos(), flats[k].yaw[m].sin());
        let mut legs = vec![
            JunctionLeg {
                curve: e.curve,
                out_dir: e.dir_out,
                half_width: e.half,
                is_end: true,
                end: e.end,
            },
            JunctionLeg { curve: k, out_dir: (-dk.0, -dk.1), half_width: other_hw, is_end: false, end: 0 },
            JunctionLeg { curve: k, out_dir: dk, half_width: other_hw, is_end: false, end: 0 },
        ];
        // the stem should point at the junction — flip if it points away
        if legs[0].out_dir.0 * (e.x - 0.0) == 0.0 && legs[0].out_dir.1 == 0.0 {
            legs.remove(0);
        }
        candidates.push(Candidate { x: e.x, z: e.z, legs });
    }

    // ---- 4. merge nearby candidates (crossing at an endpoint etc.) --
    let merge_r = (s.snap_m * 1.5).max(6.0);
    let mut merged: Vec<Candidate> = Vec::new();
    for c in candidates {
        let mut hit: Option<usize> = None;
        for (mi, m) in merged.iter().enumerate() {
            if (m.x - c.x).hypot(m.z - c.z) <= merge_r {
                hit = Some(mi);
                break;
            }
        }
        match hit {
            Some(mi) => {
                let m = &mut merged[mi];
                let n = (m.legs.len() + c.legs.len()) as f64;
                m.x = (m.x * m.legs.len() as f64 + c.x * c.legs.len() as f64) / n;
                m.z = (m.z * m.legs.len() as f64 + c.z * c.legs.len() as f64) / n;
                for l in c.legs {
                    if !m.legs.iter().any(|e| e.key() == l.key()) {
                        m.legs.push(l);
                    }
                }
            }
            None => merged.push(c),
        }
    }

    // ---- 5. classify + geometry ------------------------------------
    for c in merged {
        if c.legs.len() < 2 {
            continue;
        }
        let max_half = c.legs.iter().map(|l| l.half_width).fold(0.0f64, f64::max);
        // Roundabout only when roads MEET at their ends (no through legs) —
        // like the plugin: prefab roundabouts sit where roads terminate;
        // through crossings stay n-gon intersections (IntersectionNGon).
        let any_through = c.legs.iter().any(|l| !l.is_end);
        let kind = if s.auto_roundabout && c.legs.len() >= 3 && !any_through {
            JunctionKind::Roundabout
        } else {
            JunctionKind::Crossing
        };
        let mut legs = c.legs.clone();
        legs.sort_by(|a, b| {
            let aa = atan2f(a.out_dir.1, a.out_dir.0);
            let bb = atan2f(b.out_dir.1, b.out_dir.0);
            aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
        });
        let (outline, island, ring_outer) = match kind {
            JunctionKind::Crossing => {
                // GRG IntersectionNGon fan: per leg two corners at the stop
                // line, connected in angular order -> the classic n-gon.
                let stop = (max_half * 1.4 + 1.0).max(3.0);
                let mut outline = Vec::with_capacity(legs.len() * 2);
                for l in &legs {
                    let px = leg_perp(l.out_dir);
                    let fx = c.x + l.out_dir.0 * stop;
                    let fz = c.z + l.out_dir.1 * stop;
                    outline.push((fx - px.0 * l.half_width, fz - px.1 * l.half_width));
                    outline.push((fx + px.0 * l.half_width, fz + px.1 * l.half_width));
                }
                (outline, None, 0.0)
            }
            JunctionKind::Roundabout => {
                let ring_w = (max_half * 2.2).max(7.0);
                let outer = s.island_radius + ring_w;
                let mut outline = Vec::with_capacity(24);
                for k in 0..24 {
                    let a = k as f64 / 24.0 * std::f64::consts::TAU;
                    outline.push((c.x + a.cos() * outer, c.z + a.sin() * outer));
                }
                (outline, Some((c.x, c.z, s.island_radius)), outer)
            }
        };
        out.push(Junction { x: c.x, z: c.z, kind, legs, outline, island, ring_outer, prefab: None });
    }
    out.sort_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal));
    out
}

fn atan2f(y: f64, x: f64) -> f64 {
    y.atan2(x)
}

/* ------------------------------------------------------------------ */
/* Welding (auto join: move coincident endpoints to the cluster mean)  */
/* ------------------------------------------------------------------ */

/// Snap curve endpoints that are within `snap_m` of each other to their
/// shared mean position. Returns the number of welded joins.
pub fn weld_endpoints(curves: &mut [RoadCurve], snap_m: f64) -> usize {
    #[derive(Clone)]
    struct Ref {
        curve: usize,
        point: usize,
        x: f64,
        z: f64,
    }
    let mut refs: Vec<Ref> = Vec::new();
    for (i, c) in curves.iter().enumerate() {
        if c.closed || c.points.is_empty() {
            continue;
        }
        refs.push(Ref { curve: i, point: 0, x: c.points[0].x, z: c.points[0].z });
        let last = c.points.len() - 1;
        if last > 0 {
            refs.push(Ref { curve: i, point: last, x: c.points[last].x, z: c.points[last].z });
        }
    }
    let mut used = vec![false; refs.len()];
    let mut welded = 0usize;
    for i in 0..refs.len() {
        if used[i] {
            continue;
        }
        let mut group = vec![i];
        used[i] = true;
        for j in (i + 1)..refs.len() {
            if used[j] {
                continue;
            }
            let d = ((refs[j].x - refs[i].x).powi(2) + (refs[j].z - refs[i].z).powi(2)).sqrt();
            if d <= snap_m {
                group.push(j);
                used[j] = true;
            }
        }
        if group.len() < 2 {
            continue;
        }
        let n = group.len() as f64;
        let mx = group.iter().map(|&g| refs[g].x).sum::<f64>() / n;
        let mz = group.iter().map(|&g| refs[g].z).sum::<f64>() / n;
        for &g in &group {
            let r = &refs[g];
            curves[r.curve].points[r.point].x = mx;
            curves[r.curve].points[r.point].z = mz;
        }
        welded += 1;
    }
    welded
}

/* ------------------------------------------------------------------ */
/* 3D mesh parts                                                       */
/* ------------------------------------------------------------------ */

use crate::mesh3d::MeshPart;
use gdmath::vec3::Vec3;

/// Asphalt (or mud, for dirt roads) patch for a junction: n-gon fan for
/// crossings, ring + island for roundabouts. Textured with the
/// plain-asphalt tile (planar UVs), islands with grass. Rendered above
/// the road strips (higher y).
pub fn mesh_parts(j: &Junction) -> Vec<MeshPart> {
    mesh_parts_dirt(j, false, surface::ASPHALT, 0.0)
}

/// Painted markings only (zebra + stop lines for crossings, dashed
/// circulating-lane dividers + edge lines + give-way bars for
/// roundabouts) — used on top of GLB prefab tiles, whose own asphalt
/// carries no RoadKit-consistent markings. R9: roundabouts are no longer
/// an early-return empty vec — they get full lane-division markings.
pub fn marking_parts_for(j: &Junction, curves: &[RoadCurve]) -> Vec<MeshPart> {
    if j.kind == JunctionKind::Roundabout {
        return roundabout_marking_parts(j, curves);
    }
    if legs_unpaved(j, curves) {
        return Vec::new();
    }
    crosswalk_parts(j, junction_elev(j, curves))
}

/// Dirt/surface-aware variant: when any touching curve is an unpaved
/// road (dirt flag / MUD / GRAVEL) the patch becomes a mud surface;
/// otherwise the patch texture follows the DOMINANT SURFACE of the leg
/// curves (a CEMENT BRT corridor gives a cement patch/ring). The patch
/// rides the legs' average elevation (junction_elev).
pub fn mesh_parts_for(j: &Junction, curves: &[RoadCurve]) -> Vec<MeshPart> {
    let dirt = j.legs.iter().any(|l| {
        curves
            .get(l.curve)
            .and_then(|c| c.points.first())
            .map(|p| p.dirt)
            .unwrap_or(false)
    });
    mesh_parts_dirt(j, dirt, junction_surface(j, curves), junction_elev(j, curves))
}

fn mesh_parts_dirt(j: &Junction, dirt: bool, surf: u8, elev: f32) -> Vec<MeshPart> {
    let mut parts = Vec::new();
    // patch texture: mud when dirt, else the dominant surface (ASPHALT →
    // TEX_ASPHALT, CEMENT/CONCRETE → TEX_CEMENT, COBBLE → darker asphalt
    // fallback, GRAVEL → TEX_ROUGH, MUD → TEX_MUD)
    let (tex, tint) = if dirt {
        (crate::mesh3d::TEX_MUD, crate::mesh3d::MUD_TINT)
    } else {
        surface::tex_and_tint(surf)
    };
    let y_patch = elev + 0.03;
    let y_disc = elev + 0.06;
    match j.kind {
        JunctionKind::Crossing => {
            let n = j.outline.len();
            if n < 3 {
                return parts;
            }
            let mut part = MeshPart::new(tint);
            part.tex = tex;
            // centroid fan
            let cx = j.outline.iter().map(|p| p.0).sum::<f64>() / n as f64;
            let cz = j.outline.iter().map(|p| p.1).sum::<f64>() / n as f64;
            for &(x, z) in &j.outline {
                part.positions.push(Vec3::new(cx as f32, y_patch, cz as f32));
                part.positions.push(Vec3::new(x as f32, y_patch, z as f32));
                part.uv.push([(cx / 6.0) as f32, (cz / 6.0) as f32]);
                part.uv.push([(x / 6.0) as f32, (z / 6.0) as f32]);
            }
            for k in 0..n {
                let a = (k * 2) as u32;
                let b = a + 1;
                let c = ((k + 1) % n * 2) as u32;
                let d = c + 1;
                part.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
            parts.push(part);
            if !dirt && !surface::is_unpaved(surf) {
                parts.extend(crosswalk_parts(j, elev));
            }
        }
        JunctionKind::Roundabout => {
            let ring = 32;
            let Some((ix, iz, ir)) = j.island else { return parts };
            // annulus (surface follows the legs' dominant surface)
            let mut ann = MeshPart::new(tint);
            ann.tex = tex;
            for k in 0..ring {
                let a0 = k as f64 / ring as f64 * std::f64::consts::TAU;
                let a1 = (k + 1) as f64 / ring as f64 * std::f64::consts::TAU;
                let (x0o, z0o) = (j.x + a0.cos() * j.ring_outer, j.z + a0.sin() * j.ring_outer);
                let (x0i, z0i) = (j.x + a0.cos() * ir, j.z + a0.sin() * ir);
                let (x1o, z1o) = (j.x + a1.cos() * j.ring_outer, j.z + a1.sin() * j.ring_outer);
                let (x1i, z1i) = (j.x + a1.cos() * ir, j.z + a1.sin() * ir);
                ann.positions.push(Vec3::new(x0o as f32, y_patch, z0o as f32));
                ann.positions.push(Vec3::new(x0i as f32, y_patch, z0i as f32));
                ann.positions.push(Vec3::new(x1o as f32, y_patch, z1o as f32));
                ann.positions.push(Vec3::new(x1i as f32, y_patch, z1i as f32));
                ann.uv.push([(x0o / 6.0) as f32, (z0o / 6.0) as f32]);
                ann.uv.push([(x0i / 6.0) as f32, (z0i / 6.0) as f32]);
                ann.uv.push([(x1o / 6.0) as f32, (z1o / 6.0) as f32]);
                ann.uv.push([(x1i / 6.0) as f32, (z1i / 6.0) as f32]);
                let b = (k * 4) as u32;
                ann.indices.extend_from_slice(&[b, b + 2, b + 1, b + 1, b + 2, b + 3]);
            }
            parts.push(ann);
            // island disc (grass texture)
            let mut disc = MeshPart::new([1.2, 1.2, 1.2, 1.0]);
            disc.tex = crate::mesh3d::TEX_GRASS;
            disc.positions.push(Vec3::new(ix as f32, y_disc, iz as f32));
            disc.uv.push([(ix / 4.0) as f32, (iz / 4.0) as f32]);
            for k in 0..=ring {
                let a = k as f64 / ring as f64 * std::f64::consts::TAU;
                let x = ix + a.cos() * ir;
                let z = iz + a.sin() * ir;
                disc.positions.push(Vec3::new(x as f32, y_disc, z as f32));
                disc.uv.push([(x / 4.0) as f32, (z / 4.0) as f32]);
            }
            for k in 1..=ring {
                disc.indices.extend_from_slice(&[0, k as u32, (k + 1) as u32]);
            }
            parts.push(disc);
        }
    }
    parts
}

/// White pavement markings for a crossing: zebra crosswalk stripes + a
/// stop line across the incoming lanes of every leg (right-hand traffic).
/// Quads hover 0.03 m above the asphalt patch (painted-marking layer),
/// which itself rides the junction elevation `elev`.
fn crosswalk_parts(j: &Junction, elev: f32) -> Vec<MeshPart> {
    let mut mk = MeshPart::new([0.92, 0.92, 0.9, 1.0]);
    mk.tex = -1; // flat white
    let max_half = j
        .legs
        .iter()
        .map(|l| l.half_width)
        .fold(0.0f64, f64::max);
    let stop = (max_half * 1.4 + 1.0).max(3.0);
    let y = elev + 0.06;
    for l in &j.legs {
        let (dx, dz) = l.out_dir;
        let (px, pz) = leg_perp(l.out_dir);
        let hw = l.half_width;
        // quad helper: center dist a..b along the leg, lateral u..v across it
        let mut quad = |a: f64, b: f64, u: f64, v: f64| {
            let p = |fwd: f64, lat: f64| {
                Vec3::new(
                    (j.x + dx * fwd + px * lat) as f32,
                    y,
                    (j.z + dz * fwd + pz * lat) as f32,
                )
            };
            let base = mk.positions.len() as u32;
            mk.positions.push(p(a, u));
            mk.positions.push(p(a, v));
            mk.positions.push(p(b, u));
            mk.positions.push(p(b, v));
            mk.indices
                .extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        };
        // stop line: incoming side only. Incoming travel direction is -d;
        // right of travel = leg_perp(-d) = -leg_perp(d), so the incoming
        // lanes span lateral -hw..0 in leg_perp(d) coordinates.
        quad(stop + 0.3, stop + 0.75, -hw + 0.35, -0.15);
        // zebra: 0.45 m stripes with 0.45 m gaps, across the full width
        let z0 = stop + 1.1;
        let z1 = stop + 2.9;
        let mut lat = -hw + 0.35;
        while lat + 0.45 <= hw - 0.3 {
            quad(z0, z1, lat, lat + 0.45);
            lat += 0.9;
        }
    }
    vec![mk]
}

/* ------------------------------------------------------------------ */
/* R9: leg helpers — surface, lane count, elevation                     */
/* ------------------------------------------------------------------ */

/// The RoadPoint a leg is anchored at (endpoint legs → their end point,
/// through legs → the curve's last point as an approximation).
fn leg_point<'a>(c: &'a RoadCurve, l: &JunctionLeg) -> Option<&'a RoadPoint> {
    if c.points.is_empty() {
        return None;
    }
    if l.is_end && l.end == 0 {
        Some(&c.points[0])
    } else {
        Some(c.points.last().unwrap())
    }
}

/// True when any leg touches an unpaved road (`dirt` flag or a MUD /
/// GRAVEL surface) — such junctions get mud patches and no paint.
fn legs_unpaved(j: &Junction, curves: &[RoadCurve]) -> bool {
    j.legs.iter().any(|l| {
        curves
            .get(l.curve)
            .and_then(|c| leg_point(c, l))
            .map(|p| p.unpaved())
            .unwrap_or(false)
    })
}

/// Total lane count a leg brings to the junction (for circulating-lane
/// estimates); missing curves count as a 2-lane road.
fn leg_lane_count(curves: &[RoadCurve], l: &JunctionLeg) -> usize {
    curves
        .get(l.curve)
        .and_then(|c| leg_point(c, l))
        .map(|p| p.lanes.max(1))
        .unwrap_or(2)
}

/// Average elevation of the junction legs' anchor points (meters).
/// Through legs are sampled on the curve body at the junction position.
/// This is what lifts junction patches, islands and markings onto
/// elevated terrain (flat scenes: 0.0 → identical to the old output).
fn junction_elev(j: &Junction, curves: &[RoadCurve]) -> f32 {
    let mut sum = 0.0f64;
    let mut cnt = 0usize;
    for l in &j.legs {
        let Some(c) = curves.get(l.curve) else { continue };
        if l.is_end {
            if let Some(p) = leg_point(c, l) {
                sum += p.elev;
                cnt += 1;
            }
        } else if let Some((s, _)) = c.closest_s(j.x, j.z) {
            if let Some((pos, _)) = c.sample_at(s) {
                sum += pos.y as f64;
                cnt += 1;
            }
        }
    }
    if cnt == 0 { 0.0 } else { (sum / cnt as f64) as f32 }
}

/// Dominant surface across the junction's leg curves (mode of each
/// curve's `dominant_surface`) — drives the patch / annulus texture.
fn junction_surface(j: &Junction, curves: &[RoadCurve]) -> u8 {
    let mut counts = [0usize; surface::NAMES.len()];
    for l in &j.legs {
        if let Some(c) = curves.get(l.curve) {
            let s = c.dominant_surface() as usize;
            if s < counts.len() {
                counts[s] += 1;
            }
        }
    }
    let mut best = 0usize;
    for i in 1..counts.len() {
        if counts[i] > counts[best] {
            best = i;
        }
    }
    best as u8
}

/* ------------------------------------------------------------------ */
/* R9: roundabout lane-division markings                                */
/* ------------------------------------------------------------------ */

/// One arc-slice quad of a painted roundabout ring (thin, flat, trim-
/// sheet sampled). Vertex/index layout matches the crossing stop-line
/// quad helper: (a0,outer) (a0,inner) (a1,outer) (a1,inner) with the
/// [0,2,1 | 1,2,3] index pattern, so the facing matches all other
/// painted markings.
#[allow(clippy::too_many_arguments)]
fn push_arc_quad(
    paint: &mut MeshPart,
    cx: f64,
    cz: f64,
    y: f32,
    r: f64,
    a0: f64,
    a1: f64,
    half_w: f64,
    u0: f32,
    u1: f32,
    v: f32,
) {
    // walking the circle with increasing angle (CCW), the inward normal
    // is the right of travel — lateral offsets: -w = outer, +w = inner
    let pt = |a: f64, lat: f64| {
        let (px, pz) = (-a.sin(), a.cos());
        Vec3::new(
            (cx + a.cos() * r + px * lat) as f32,
            y,
            (cz + a.sin() * r + pz * lat) as f32,
        )
    };
    let base = paint.positions.len() as u32;
    paint.positions.push(pt(a0, -half_w));
    paint.positions.push(pt(a0, half_w));
    paint.positions.push(pt(a1, -half_w));
    paint.positions.push(pt(a1, half_w));
    paint.uv.push([u0, v]);
    paint.uv.push([u1, v]);
    paint.uv.push([u0, v]);
    paint.uv.push([u1, v]);
    paint
        .indices
        .extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
}

/// Emit a painted ring arc: solid when `dash` is None, otherwise a
/// 2.2 m dash / 2.6 m gap pattern. Slices are ~1.2° so the paint bends
/// with the circle; V accumulates arc length for trim tiling.
#[allow(clippy::too_many_arguments)]
fn emit_ring_arc(
    paint: &mut MeshPart,
    cx: f64,
    cz: f64,
    y: f32,
    r: f64,
    half_w: f64,
    dash: Option<(f64, f64)>,
    u0: f32,
    u1: f32,
    v_tile: f64,
) {
    if r <= half_w * 2.0 {
        return;
    }
    let step = 1.2f64.to_radians();
    let mut quad = |a0: f64, a1: f64, s: f64| {
        push_arc_quad(paint, cx, cz, y, r, a0, a1, half_w, u0, u1, (s / v_tile.max(0.5)) as f32);
    };
    match dash {
        None => {
            let slices = (std::f64::consts::TAU / step).ceil() as usize;
            for k in 0..slices {
                let a0 = k as f64 * std::f64::consts::TAU / slices as f64;
                let a1 = (k + 1) as f64 * std::f64::consts::TAU / slices as f64;
                quad(a0, a1, (a0 + a1) * 0.5 * r);
            }
        }
        Some((dash_m, gap_m)) => {
            let mut a = 0.0f64;
            while a < std::f64::consts::TAU {
                let dash_ang = (dash_m / r).min(std::f64::consts::TAU - a);
                if dash_ang <= 1e-6 {
                    break;
                }
                let slices = ((dash_ang / step).ceil() as usize).max(1);
                for k in 0..slices {
                    let s0 = a + dash_ang * k as f64 / slices as f64;
                    let s1 = a + dash_ang * (k + 1) as f64 / slices as f64;
                    quad(s0, s1, (s0 + s1) * 0.5 * r);
                }
                a += dash_ang + gap_m / r;
            }
        }
    }
}

/// Roundabout painted markings (R9):
/// - one DASHED divider circle per circulating lane (2.2 m dash / 2.6 m
///   gap, 0.12 m wide, COL_SINGLE_LINE trim column, TEX_ROAD_TRIM) at
///   radii evenly dividing the lane band (island + 1.2 .. outer - 0.6),
/// - a solid outer edge line (outer - 0.35, 0.15 m) and a solid island
///   edge line (island + 0.35),
/// - a give-way (yield) bar pair across the entering lanes of every leg,
///   reusing the crossing stop-line quad style (flat white), oriented
///   radially.
/// The circulating lane count is the mean leg lane count (rounded,
/// clamped 1..=3). Markings hover 0.115 m above the ground plane — above
/// both the procedural annulus (y +0.03) and WFC prefab tiles scaled up
/// to 3x (ring top ~y +0.10), so the dashes never z-fight.
pub fn roundabout_marking_parts(j: &Junction, curves: &[RoadCurve]) -> Vec<MeshPart> {
    let mut out: Vec<MeshPart> = Vec::new();
    let Some((ix, iz, ir)) = j.island else { return out };
    if j.legs.is_empty() || j.ring_outer <= ir + 1.0 {
        return out;
    }
    if legs_unpaved(j, curves) {
        return out; // dirt junctions carry no paint (matches crossings)
    }
    let y = junction_elev(j, curves) + 0.115;

    // circulating lane count: mean leg lanes, clamped 1..=3
    let sum: usize = j.legs.iter().map(|l| leg_lane_count(curves, l)).sum();
    let n = ((sum as f64 / j.legs.len() as f64).round() as usize).clamp(1, 3);

    // lane band between the island apron and the outer entry margin
    let band_in = ir + 1.2;
    let band_out = (j.ring_outer - 0.6).max(band_in + 0.5);

    // trim-sheet paint: dashed dividers + solid edge lines
    let mut paint = MeshPart::new(crate::mesh3d::PAINT_TINT);
    paint.tex = crate::mesh3d::TEX_ROAD_TRIM;
    let u0 = (crate::mesh3d::COL_SINGLE_LINE as f64 * crate::mesh3d::UV_COL) as f32;
    let u1 = ((crate::mesh3d::COL_SINGLE_LINE + 1) as f64 * crate::mesh3d::UV_COL) as f32;
    // V scale: like lane strips, one texture repeat per 4 × lane width
    let widths: Vec<f64> = j
        .legs
        .iter()
        .filter_map(|l| curves.get(l.curve).and_then(|c| leg_point(c, l)))
        .map(|p| p.lane_width.max(1.0))
        .collect();
    let lw = if widths.is_empty() {
        3.5
    } else {
        widths.iter().sum::<f64>() / widths.len() as f64
    };
    let v_tile = 4.0 * lw;

    for i in 1..n {
        let r = band_in + (band_out - band_in) * i as f64 / n as f64;
        emit_ring_arc(&mut paint, ix, iz, y, r, 0.06, Some((2.2, 2.6)), u0, u1, v_tile);
    }
    emit_ring_arc(&mut paint, ix, iz, y, j.ring_outer - 0.35, 0.075, None, u0, u1, v_tile);
    emit_ring_arc(&mut paint, ix, iz, y, ir + 0.35, 0.075, None, u0, u1, v_tile);
    out.push(paint);

    // give-way bars at each entry (flat white, crossing stop-line style,
    // adapted to radial orientation: fwd = outward along the leg)
    let mut yld = MeshPart::new([0.92, 0.92, 0.9, 1.0]);
    yld.tex = -1;
    for l in &j.legs {
        let (dx, dz) = l.out_dir;
        let (px, pz) = leg_perp(l.out_dir);
        let hw = l.half_width.max(1.0);
        let f0 = j.ring_outer - 1.7;
        let f1 = j.ring_outer - 1.25;
        let lat0 = -hw + 0.35; // incoming lanes span lat -hw..0 (like crossings)
        let lat1 = -0.15;
        if lat1 - lat0 < 0.5 {
            continue;
        }
        let mid = (lat0 + lat1) / 2.0;
        let mut quad = |fw0: f64, fw1: f64, u: f64, v: f64| {
            let p = |fw: f64, lt: f64| {
                Vec3::new(
                    (j.x + dx * fw + px * lt) as f32,
                    y,
                    (j.z + dz * fw + pz * lt) as f32,
                )
            };
            let base = yld.positions.len() as u32;
            yld.positions.push(p(fw0, u));
            yld.positions.push(p(fw0, v));
            yld.positions.push(p(fw1, u));
            yld.positions.push(p(fw1, v));
            yld
                .indices
                .extend_from_slice(&[base, base + 2, base + 1, base + 1, base + 2, base + 3]);
        };
        // dashed pair: two quads with a lateral gap across the entry
        quad(f0, f1, lat0, mid - 0.15);
        quad(f0, f1, mid + 0.15, lat1);
    }
    out.push(yld);
    out
}
