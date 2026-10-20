//! GRG-style procedural road surface meshes (meters space) — TEXTURED.
//!
//! Mirrors godot-road-generator's material model exactly: the road surface
//! is generated at runtime by sweeping cross-section strips along the
//! tessellated RoadPoint curve, and every lane strip samples one column of
//! the plugin's TRIM SHEET (road_texture, 8 columns of `uv_width = 0.125`):
//!
//!   col 0 SHOULDER · 1 SLOW (solid+dotted white) · 2 MIDDLE (dotted both)
//!   3 FAST (double-yellow side) · 4 TWO_WAY (yellow+white) · 5 ONE_WAY
//!   6 SINGLE_LINE · 7 NO_MARKING (plain asphalt)
//!
//! V tiles every `4 × lane_width` meters along the road (GRG's
//! `single_uv_height = min_road_width * 4`) so dashes stay seamless.
//! Median / pathway / junction / ground use dedicated textures.
//!
//! R7 surface modes:
//! - `RoadPoint.dirt` → mud lane surface (U across the paved width, V
//!   tiled every 6 lane widths) with rough-gravel shoulders; trim-sheet
//!   markings are skipped for those strips.
//! - medians >= 3 m are BRT cast-concrete (cement.raw) with a 0.4 m
//!   tactile (red) edge line on both inner edges; narrower medians keep
//!   median.raw. Pathways always stay sidewalk.raw.
//!
//! R9 additions (gutters / underside / elevation / surface types):
//! - `RoadPoint.gutter_profile` now generates V-gutter aprons OUTSIDE
//!   each shoulder (sidewalk texture, GUTTER_TINT) sloping down by
//!   `gutter_profile.1` over `gutter_profile.0` meters, tapering with
//!   the cross-section; pathways shift outward behind the gutter.
//!   Skipped on unpaved samples (`dirt` flag or MUD/GRAVEL surface) and
//!   when `gutter_profile.0 <= 0`. `gutter_parts()` returns just the
//!   gutter + underside parts so renderers can toggle them.
//! - `RoadPoint.underside_thickness > 0` emits a flat slab under the
//!   full corridor at `elev - thickness` (COL_PLAIN trim fallback).
//! - every strip rides the interpolated `RoadPoint.elev`; with the
//!   default elev = 0 the output is identical to the flat mesh.

use crate::model::RoadCurve;
use gdmath::vec2::Vec2;
use gdmath::vec3::Vec3;

#[derive(Clone, Debug)]
pub struct MeshPart {
    pub positions: Vec<Vec3>,
    pub indices: Vec<u32>,
    /// rgba 0..1 (tint; textures multiply over this)
    pub color: [f32; 4],
    /// per-vertex uv (same count as positions) when textured
    pub uv: Vec<[f32; 2]>,
    /// texture slot (roadkit::textures); -1 = flat color only
    pub tex: i32,
}

impl MeshPart {
    pub fn new(color: [f32; 4]) -> Self {
        Self { positions: Vec::new(), indices: Vec::new(), color, uv: Vec::new(), tex: -1 }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct MeshQuality {
    /// include lane markings / edge lines (trim-sheet lane strips)
    pub markings: bool,
    /// curve sampling density in meters
    pub sample_m: f64,
}

impl Default for MeshQuality {
    fn default() -> Self {
        Self { markings: true, sample_m: 4.0 }
    }
}

pub const ASPHALT: [u8; 3] = [70, 74, 82];
pub const ASPHALT_LIGHT: [u8; 3] = [84, 88, 96];
pub const MARKING_WHITE: [u8; 3] = [235, 238, 242];
pub const MARKING_YELLOW: [u8; 3] = [250, 204, 60];
pub const CONCRETE: [u8; 3] = [154, 158, 164];
pub const PATHWAY: [u8; 3] = [176, 170, 160];
pub const ISLAND_GREEN: [u8; 3] = [86, 138, 90];

/// Trim-sheet UV columns (GRG `uv_width = 0.125`, `uv_mul` mapping).
pub const UV_COL: f64 = 0.125;
pub const COL_SHOULDER: usize = 0;
pub const COL_SLOW: usize = 1;
pub const COL_MIDDLE: usize = 2;
pub const COL_FAST: usize = 3;
pub const COL_TWO_WAY: usize = 4;
pub const COL_ONE_WAY: usize = 5;
pub const COL_SINGLE_LINE: usize = 6;
pub const COL_PLAIN: usize = 7;

pub const TEX_ROAD_TRIM: i32 = 0;
pub const TEX_GRASS: i32 = 1;
pub const TEX_SIDEWALK: i32 = 2;
pub const TEX_MEDIAN: i32 = 3;
pub const TEX_ASPHALT: i32 = 4;
pub const TEX_CEMENT: i32 = 5;
pub const TEX_MUD: i32 = 6;
pub const TEX_ROUGH: i32 = 7;
pub const TEX_TACTILE: i32 = 8;

pub const ROAD_TINT: [f32; 4] = [1.55, 1.55, 1.55, 1.0];
pub const CONCRETE_TINT: [f32; 4] = [1.35, 1.35, 1.35, 1.0];
pub const SIDEWALK_TINT: [f32; 4] = [1.25, 1.25, 1.25, 1.0];
/// Dirt-mode lane surface (warm; mud.raw carries the brown).
pub const MUD_TINT: [f32; 4] = [1.10, 1.00, 0.85, 1.0];
/// Dirt-mode shoulder gravel (mid grey-brown lift).
pub const ROUGH_TINT: [f32; 4] = [1.20, 1.15, 1.05, 1.0];
/// BRT tactile edge line over the terracotta texture.
pub const TACTILE_TINT: [f32; 4] = [1.10, 1.00, 0.95, 1.0];
/// V-gutter apron tint — sidewalk texture, slightly darker than pathways.
pub const GUTTER_TINT: [f32; 4] = [1.05, 1.05, 1.05, 1.0];
/// Underside slab target color (dark concrete-ish; see UNDERSIDE_TINT).
pub const UNDERSIDE: [u8; 3] = [90, 90, 94];
/// Underside slab tint: applied over the trim sheet's COL_PLAIN column
/// (flat ~0.25 grey asphalt) this lands on the UNDERSIDE tone.
pub const UNDERSIDE_TINT: [f32; 4] = [1.42, 1.42, 1.48, 1.0];
/// Painted-line tint for trim-sampled roundabout markings — bright
/// enough to read as white paint even where the trim column is plain.
pub const PAINT_TINT: [f32; 4] = [2.6, 2.6, 2.6, 1.0];

/// Medians at least this wide are BRT cast-concrete slabs: cement.raw
/// surface + tactile edge lines (instead of median.raw).
pub const BRT_MEDIAN_M: f64 = 3.0;
/// Tactile edge-line strip width on a BRT median (meters).
pub const TACTILE_W: f64 = 0.4;

/// Low-quality (flat) approximations of the new surfaces.
pub const MUD_FLAT: [u8; 3] = [117, 90, 62];
pub const ROUGH_FLAT: [u8; 3] = [120, 112, 100];
pub const TACTILE_FLAT: [u8; 3] = [188, 86, 66];
pub const CEMENT_FLAT: [u8; 3] = [172, 176, 182];

pub fn rgb(hex: [u8; 3], alpha: f32) -> [f32; 4] {
    [hex[0] as f32 / 255.0, hex[1] as f32 / 255.0, hex[2] as f32 / 255.0, alpha]
}

/// Per-vertex right-of-travel perpendicular for a polyline in (x, z).
fn perps(pts: &[Vec2]) -> Vec<Vec2> {
    let n = pts.len();
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let dir = if n >= 2 {
            if i == 0 {
                pts[1] - pts[0]
            } else if i == n - 1 {
                pts[n - 1] - pts[n - 2]
            } else {
                pts[i + 1] - pts[i - 1]
            }
        } else {
            Vec2::new(1.0, 0.0)
        };
        let dir = dir.normalized();
        out.push(Vec2::new(-dir.y, dir.x));
    }
    out
}

/// Cumulative arc length per polyline vertex.
fn cumulative(pts: &[Vec2]) -> Vec<f64> {
    let mut cum = Vec::with_capacity(pts.len());
    cum.push(0.0);
    for i in 1..pts.len() {
        cum.push(cum[i - 1] + pts[i].dist(pts[i - 1]));
    }
    cum
}

/// Thin painted line at lateral `offset_m` (right-positive), optional dash.
/// Kept for the 2D/overlay helpers and low-quality paths.
#[allow(dead_code)]
fn paint_line(
    pts: &[Vec2],
    offset_m: f64,
    width_m: f64,
    y: f32,
    color: [f32; 4],
    dash: Option<(f64, f64)>,
) -> MeshPart {
    let mut part = MeshPart::new(color);
    let cum = cumulative(pts);
    let total = *cum.last().unwrap_or(&0.0);
    if total < 0.5 {
        return part;
    }

    let per = perps(pts);
    let emit = |d0: f64, d1: f64, part: &mut MeshPart, per: &[Vec2]| {
        let n = 2.max(((d1 - d0) / 3.0).ceil() as usize);
        // locate arc position by linear scan (cum is monotonic)
        let at = |d: f64| -> (Vec2, usize) {
            let mut i = 1usize;
            while i < cum.len() - 1 && cum[i] < d {
                i += 1;
            }
            let seg = (cum[i] - cum[i - 1]).max(1e-9);
            let t = ((d - cum[i - 1]) / seg).clamp(0.0, 1.0);
            let p = pts[i - 1].lerp(pts[i], t);
            (p, i)
        };
        for k in 0..=n {
            let d = d0 + (d1 - d0) * (k as f64 / n as f64);
            let (p, i) = at(d);
            let right = per[i.min(per.len() - 1)];
            let c = p + right * offset_m;
            let hw = width_m / 2.0;
            part.positions.push(Vec3::new((c.x - right.x * hw) as f32, y, (c.y - right.y * hw) as f32));
            part.positions.push(Vec3::new((c.x + right.x * hw) as f32, y, (c.y + right.y * hw) as f32));
        }
        for k in 0..n {
            let a = (k * 2) as u32;
            part.indices.extend_from_slice(&[a, a + 2, a + 1, a + 1, a + 2, a + 3]);
        }
    };

    match dash {
        None => emit(0.0, total, &mut part, &per),
        Some((dash_m, gap_m)) => {
            let mut d = 0.0;
            while d < total {
                let d1 = (d + dash_m).min(total);
                emit(d, d1, &mut part, &per);
                d = d1 + gap_m;
            }
        }
    }
    part
}

/* ------------------------------------------------------------------ */
/* Sweep profile (shared by build_parts / gutter_parts)                */
/* ------------------------------------------------------------------ */

/// Per-sample sweep profile: the flattened curve plus every cross-section
/// property lerped between the bounding RoadPoints (GRG: "All RoadPoint
/// settings on a single RoadPoint are specific to itself, and will be
/// interpolated between two RoadPoints"). R9 adds elevation, the gutter
/// profile and the effective underside thickness to the same lerp.
struct Sweep {
    pts: Vec<Vec2>,
    cum: Vec<f64>,
    n: usize,
    seg_of: Vec<usize>,
    seg_t: Vec<f64>,
    seg_start: Vec<usize>,
    half: Vec<f64>,
    m_half: Vec<f64>,
    sh_l: Vec<f64>,
    sh_r: Vec<f64>,
    pw_l: Vec<f64>,
    pw_r: Vec<f64>,
    w_lft: Vec<f64>,
    w_rgt: Vec<f64>,
    /// interpolated RoadPoint.elev per sample (meters)
    elev: Vec<f64>,
    /// gutter apron width per sample (gutter_profile.0; <= 0 = none)
    gut_w: Vec<f64>,
    /// gutter drop per sample (gutter_profile.1, negative = below grade)
    gut_y: Vec<f64>,
    /// unpaved per sample (dirt flag or MUD/GRAVEL surface) — no gutters
    unpaved: Vec<bool>,
    /// effective underside thickness per sample (0 = none)
    unders: Vec<f64>,
    /// right-of-travel basis per sample. GRG vector alignment: at segment
    /// boundaries this is the RoadPoint's OWN bearing (exact transform
    /// basis.x), mid-curve it follows the local tangent — so two segments
    /// sharing a RoadPoint meet on the same cross-section with no kink.
    per: Vec<Vec2>,
    /// per-sample V coordinate for TRIM-sheet strips (GRG UV tiling: per
    /// segment the V span is snapped to half tiles so dashes stay seamless
    /// across RoadPoints).
    v_snap: Vec<f32>,
    v_tile: f64,
    v_dirt: f64,
    dirt: bool,
}

fn sweep_profile(curve: &RoadCurve, q: &MeshQuality) -> Sweep {
    let n_points = curve.points.len();
    if n_points < 2 {
        return Sweep {
            pts: Vec::new(),
            cum: Vec::new(),
            n: 0,
            seg_of: Vec::new(),
            seg_t: Vec::new(),
            seg_start: Vec::new(),
            half: Vec::new(),
            m_half: Vec::new(),
            sh_l: Vec::new(),
            sh_r: Vec::new(),
            pw_l: Vec::new(),
            pw_r: Vec::new(),
            w_lft: Vec::new(),
            w_rgt: Vec::new(),
            elev: Vec::new(),
            gut_w: Vec::new(),
            gut_y: Vec::new(),
            unpaved: Vec::new(),
            unders: Vec::new(),
            per: Vec::new(),
            v_snap: Vec::new(),
            v_tile: 14.0,
            v_dirt: 21.0,
            dirt: false,
        };
    }
    let seg_count = if curve.closed { n_points } else { n_points - 1 };
    let mut pts: Vec<Vec2> = Vec::new();
    let mut cum: Vec<f64> = Vec::new();
    let mut seg_of: Vec<usize> = Vec::new(); // per sample: owning segment
    let mut seg_t: Vec<f64> = Vec::new(); // per sample: t along that segment
    let mut seg_start: Vec<usize> = Vec::new(); // first sample index per segment
    let mut seg_lens: Vec<f64> = Vec::new(); // per segment: curve length (m)
    for k in 0..seg_count {
        let a = &curve.points[k];
        let b = &curve.points[(k + 1) % n_points];
        let seg = crate::curve::segment_cubic(a, b);
        let (sp, sc, seg_len) = crate::curve::flatten_cubic(&seg, q.sample_m.max(1.0));
        let base = cum.last().copied().unwrap_or(0.0);
        seg_start.push(pts.len());
        seg_lens.push(seg_len);
        for (j, p) in sp.iter().enumerate() {
            if j == 0 && !pts.is_empty() {
                continue; // shared boundary with the previous segment
            }
            pts.push(*p);
            cum.push(base + sc[j]);
            seg_of.push(k);
            seg_t.push(if seg_len > 1e-9 { (sc[j] / seg_len).clamp(0.0, 1.0) } else { 0.0 });
        }
    }
    let n = pts.len();

    let mut half = vec![0.0f64; n];
    let mut m_half = vec![0.0f64; n];
    let mut sh_l = vec![0.0f64; n];
    let mut sh_r = vec![0.0f64; n];
    let mut pw_l = vec![0.0f64; n];
    let mut pw_r = vec![0.0f64; n];
    let mut w_lft = vec![0.0f64; n]; // left lane-area width (median edge → shoulder line)
    let mut w_rgt = vec![0.0f64; n];
    let mut elev = vec![0.0f64; n];
    let mut gut_w = vec![0.0f64; n];
    let mut gut_y = vec![0.0f64; n];
    let mut unpaved = vec![false; n];
    let mut unders = vec![0.0f64; n];

    let mut min_lw = f64::INFINITY;
    for p in &curve.points {
        min_lw = min_lw.min(p.lane_width.max(1.0));
    }
    let v_tile = 4.0 * min_lw; // GRG single_uv_height (constant per curve)
    let dirt = curve.points[0].dirt;
    let v_dirt = 6.0 * min_lw;
    // underside: an explicit positive RoadPoint thickness wins, else the
    // container value; -1 (default) = auto/none → 0 (no slab)
    let container_th = if curve.underside_thickness > 0.0 { curve.underside_thickness } else { 0.0 };

    for i in 0..n {
        let k = seg_of[i];
        let a = &curve.points[k];
        let b = &curve.points[(k + 1) % n_points];
        let t = seg_t[i];
        // GRG parity: lane-area widths + underside thickness interpolate with
        // `ease(offset, smooth_amount)`, smooth_amount = -2 (pins the ends so
        // the cross-section lands exactly on each RoadPoint's layout);
        // shoulders / gutters / pathways use the raw offset (GRG lines
        // 1186-1191 use the un-eased offset for those).
        let te = crate::curve::ease_neg2(t);
        let lp = |x: f64, y: f64| x + (y - x) * t;   // raw-offset lerp
        let lpe = |x: f64, y: f64| x + (y - x) * te; // eased lerp (GRG)
        let lane_a = a.lanes.max(1) as f64 * a.lane_width;
        let lane_b = b.lanes.max(1) as f64 * b.lane_width;
        let med = lpe(a.median, b.median);
        let shl = lp(a.shoulder_l, b.shoulder_l);
        let shr = lp(a.shoulder_r, b.shoulder_r);
        half[i] = (lpe(lane_a, lane_b) + med + shl + shr) / 2.0;
        m_half[i] = med / 2.0;
        sh_l[i] = shl;
        sh_r[i] = shr;
        pw_l[i] = lp(a.pathway_l, b.pathway_l);
        pw_r[i] = lp(a.pathway_r, b.pathway_r);
        w_lft[i] = (half[i] - shl - med / 2.0).max(0.05);
        w_rgt[i] = (half[i] - shr - med / 2.0).max(0.05);
        elev[i] = lp(a.elev, b.elev);
        gut_w[i] = lp(a.gutter_profile.0, b.gutter_profile.0).max(0.0);
        gut_y[i] = lp(a.gutter_profile.1, b.gutter_profile.1);
        unpaved[i] = a.unpaved() || b.unpaved();
        let pt = lpe(a.underside_thickness, b.underside_thickness);
        unders[i] = if pt > 0.0 { pt } else { container_th };
    }

    // GRG vector alignment (`_normal_for_offset_eased`): at a segment end the
    // cross-section basis is the RoadPoint's OWN transform (exact bearing),
    // never a chord approximation — so geometry from both sides of a shared
    // RoadPoint lines up pixel-perfect and intersections join seamlessly.
    // Mid-curve the basis follows the local tangent (finite difference of the
    // baked curve, as in GRG).
    let mut per = perps(&pts);
    if n > 0 {
        let d0 = curve.points[0].dir();
        per[0] = Vec2::new(-d0.1, d0.0);
        for i in 0..n {
            if seg_t[i] >= 1.0 - 1e-9 {
                let bp = &curve.points[(seg_of[i] + 1) % n_points];
                let d = bp.dir();
                per[i] = Vec2::new(-d.1, d.0);
            }
        }
    }

    // GRG UV tiling (road_segment.gd `_build_geo`): per segment the V span is
    // `target_uv_tiles = seg_len / single_uv_height` with
    // `single_uv_height = min(lane_width) * 4`, snapped to half-tile steps so
    // the dash pattern lands on the same phase at the next RoadPoint.
    let mut v_snap = vec![0.0f32; n];
    for i in 0..n {
        let k = seg_of[i];
        let a = &curve.points[k];
        let b = &curve.points[(k + 1) % n_points];
        let uv_h = (4.0 * a.lane_width.min(b.lane_width)).max(0.5);
        let target = seg_lens[k] / uv_h;
        let snapped = if target < 0.3 {
            target // no snapping, UVs slide freely without stretching
        } else if target < 0.75 {
            0.5
        } else if target < 1.25 {
            1.0
        } else if target < 1.75 {
            1.5
        } else {
            target.round()
        };
        v_snap[i] = (snapped * seg_t[i]) as f32;
    }

    Sweep {
        pts,
        cum,
        n,
        seg_of,
        seg_t,
        seg_start,
        half,
        m_half,
        sh_l,
        sh_r,
        pw_l,
        pw_r,
        w_lft,
        w_rgt,
        elev,
        gut_w,
        gut_y,
        unpaved,
        unders,
        per,
        v_snap,
        v_tile,
        v_dirt,
        dirt,
    }
}

/// Active gutter width at a sample (0 when skipped: unpaved sample or
/// `gutter_profile.0 <= 0` — the emitters drop collapsed quads).
fn gutter_w_at(sw: &Sweep, i: usize) -> f64 {
    if sw.unpaved[i] || sw.gut_w[i] <= 1e-4 {
        0.0
    } else {
        sw.gut_w[i]
    }
}

/// R9 gutter + underside geometry: V-gutter aprons OUTSIDE each shoulder
/// (sidewalk texture, GUTTER_TINT) sloping from the road edge down by
/// `gutter_profile.1` over `gutter_profile.0` meters — tapering with the
/// cross-section, skipped on unpaved samples (GRG-style: dirt roads have
/// no gutters) — plus a flat underside slab under the full corridor at
/// `elev - underside_thickness` when positive (COL_PLAIN trim fallback,
/// dark concrete tint, downward-facing winding). Shared by build_parts
/// and exposed standalone via `gutter_parts`.
fn gutter_underside_parts(sw: &Sweep) -> Vec<MeshPart> {
    let mut parts = Vec::new();
    if sw.n < 2 {
        return parts;
    }
    let per = &sw.per;

    /* ---- V-gutter aprons, before the pathways in the cross-section ---- */
    if (0..sw.n).any(|i| gutter_w_at(sw, i) > 0.0) {
        for left in [true, false] {
            let mut part = MeshPart::new(GUTTER_TINT);
            part.tex = TEX_SIDEWALK;
            let mut active = false;
            for i in 0..sw.n {
                let g = gutter_w_at(sw, i);
                let inner = if left { -sw.half[i] } else { sw.half[i] };
                let outer = if left { -sw.half[i] - g } else { sw.half[i] + g };
                // strip convention: first vertex = smaller lateral offset
                let (l_off, r_off) = if left { (outer, inner) } else { (inner, outer) };
                let y_in = sw.elev[i] as f32 + 0.02; // meets the shoulder top
                let y_out = (sw.elev[i] + sw.gut_y[i]) as f32; // V-gutter drop
                let (y_l, y_r) = if left { (y_out, y_in) } else { (y_in, y_out) };
                let pl = per[i] * l_off;
                let pr = per[i] * r_off;
                part
                    .positions
                    .push(Vec3::new((sw.pts[i].x + pl.x) as f32, y_l, (sw.pts[i].y + pl.y) as f32));
                part
                    .positions
                    .push(Vec3::new((sw.pts[i].x + pr.x) as f32, y_r, (sw.pts[i].y + pr.y) as f32));
                let v = (sw.cum[i] / 2.0) as f32; // sidewalk texture tiling
                part.uv.push([0.0, v]);
                part.uv.push([1.0, v]);
                if g > 0.0 {
                    active = true;
                }
            }
            if !active {
                continue;
            }
            for i in 0..sw.n - 1 {
                if gutter_w_at(sw, i) <= 0.0 && gutter_w_at(sw, i + 1) <= 0.0 {
                    continue; // gutter disabled on this stretch
                }
                let a = (i * 2) as u32;
                let b = a + 1;
                let c = (i * 2 + 2) as u32;
                let d = (i * 2 + 3) as u32;
                part.indices.extend_from_slice(&[a, c, b, b, c, d]);
            }
            parts.push(part);
        }
    }

    /* ---- underside slab (underside_thickness > 0) ---- */
    if sw.unders.iter().any(|&x| x > 1e-6) {
        let mut slab = MeshPart::new(UNDERSIDE_TINT);
        slab.tex = TEX_ROAD_TRIM;
        let u0 = (COL_PLAIN as f64 * UV_COL) as f32;
        let u1 = ((COL_PLAIN + 1) as f64 * UV_COL) as f32;
        for i in 0..sw.n {
            let g = gutter_w_at(sw, i);
            let pl = per[i] * (-sw.half[i] - sw.pw_l[i] - g);
            let pr = per[i] * (sw.half[i] + sw.pw_r[i] + g);
            let y = (sw.elev[i] - sw.unders[i]) as f32;
            slab
                .positions
                .push(Vec3::new((sw.pts[i].x + pl.x) as f32, y, (sw.pts[i].y + pl.y) as f32));
            slab
                .positions
                .push(Vec3::new((sw.pts[i].x + pr.x) as f32, y, (sw.pts[i].y + pr.y) as f32));
            let v = (sw.cum[i] / 6.0) as f32;
            slab.uv.push([u0, v]);
            slab.uv.push([u1, v]);
        }
        for i in 0..sw.n - 1 {
            if sw.unders[i] <= 1e-6 && sw.unders[i + 1] <= 1e-6 {
                continue;
            }
            let a = (i * 2) as u32;
            let b = a + 1;
            let c = (i * 2 + 2) as u32;
            let d = (i * 2 + 3) as u32;
            // the underside faces DOWN: reversed winding of the up-facing
            // [a, c, b | b, c, d] pattern
            slab.indices.extend_from_slice(&[a, b, c, b, d, c]);
        }
        parts.push(slab);
    }
    parts
}

/// Only the gutter + underside geometry for a curve (no road surface) —
/// lets renderers toggle drainage / underside parts independently of
/// `build_parts`.
pub fn gutter_parts(curve: &RoadCurve, quality: MeshQuality) -> Vec<MeshPart> {
    if curve.points.len() < 2 {
        return Vec::new();
    }
    let sw = sweep_profile(curve, &quality);
    gutter_underside_parts(&sw)
}

/// Build the full TEXTURED road surface for a curve.
///
/// GRG RULE (wiki, step 6): "All RoadPoint settings on a single RoadPoint
/// are specific to itself, and will be interpolated between two RoadPoints."
/// Every cross-section property (lanes, lane width, shoulders, median,
/// pathways, ELEVATION, gutter profile) is lerped along each segment; when
/// the lane count differs between two points the extra OUTER lane wedges
/// out linearly — a real lane-drop taper ("4 lanes here, 2 in that
/// section, joined well"). Lane strips sample the plugin's TRIM SHEET
/// columns:
///
///   col 0 SHOULDER · 1 SLOW (solid+dotted white) · 2 MIDDLE (dotted both)
///   3 FAST (double-yellow side) · 4 TWO_WAY (yellow+white) · 5 ONE_WAY
///   6 SINGLE_LINE · 7 NO_MARKING (plain asphalt)
///
/// V tiles every `4 × min_lane_width` meters along the whole curve (GRG's
/// `single_uv_height = min_road_width * 4`), so dashes stay seamless across
/// tapers. Dirt roads sweep mud + rough gravel strips instead; medians
/// >= 3 m are BRT cast-concrete with tactile edge lines.
///
/// R9: the sweep also emits gutter aprons + the underside slab (see
/// `gutter_underside_parts`; pathways shift outward behind the gutter),
/// and every strip rides the interpolated RoadPoint.elev — with the
/// default elev = 0 the output is identical to the flat mesh.
pub fn build_parts(curve: &RoadCurve, q: MeshQuality) -> Vec<MeshPart> {
    if curve.points.len() < 2 {
        return Vec::new();
    }

    /* ---- 1+2. flatten + per-sample lerped profile (see `Sweep`) ------- */
    let sw = sweep_profile(curve, &q);
    let mut parts: Vec<MeshPart> = gutter_underside_parts(&sw); // before pathways
    parts.extend(pier_parts(curve)); // flyover supports (JICA grade separation)
    let Sweep {
        pts,
        cum,
        n,
        seg_of,
        seg_t,
        seg_start,
        half,
        m_half,
        sh_l,
        sh_r,
        pw_l,
        pw_r,
        w_lft,
        w_rgt,
        elev,
        gut_w,
        unpaved,
        per,
        v_snap,
        v_tile,
        v_dirt,
        dirt,
        ..
    } = sw;
    let n_points = curve.points.len();
    let seg_count = if curve.closed { n_points } else { n_points - 1 };

    // discrete per-side lane counts at each SEGMENT endpoint layout
    let split = |ln: i32, oneway: bool| -> (usize, usize) {
        let ln = ln.max(1);
        if oneway {
            (0, ln as usize)
        } else {
            let l = (ln / 2) as usize;
            let r = (ln - l as i32).max(1) as usize;
            (l, r)
        }
    };

    // lane-area outer edges per sample (lateral offsets, right-positive)
    let lane_span_l = |i: usize| -half[i] + sh_l[i];
    let lane_span_r = |i: usize| half[i] - sh_r[i];

    /* ---- 3. strip builder with PER-SAMPLE offsets (the taper primitive) */
    #[allow(clippy::too_many_arguments)]
    fn strip_dyn(
        pts: &[Vec2],
        per: &[Vec2],
        cum: &[f64],
        elev: &[f64],
        off_l: &[f64],
        off_r: &[f64],
        y_off: f32,
        color: [f32; 4],
        tex: i32,
        u_ls: &[f32],
        u_rs: &[f32],
        v_tile: f64,
        v_override: Option<&[f32]>,
    ) -> MeshPart {
        let mut part = MeshPart::new(color);
        part.tex = tex;
        if pts.len() < 2 {
            return part;
        }
        for (i, p) in pts.iter().enumerate() {
            let rl = per[i] * off_l[i];
            let rr = per[i] * off_r[i];
            // R9: y rides the interpolated RoadPoint.elev + strip offset
            part.positions.push(Vec3::new(
                (p.x + rl.x) as f32,
                elev[i] as f32 + y_off,
                (p.y + rl.y) as f32,
            ));
            part.positions.push(Vec3::new(
                (p.x + rr.x) as f32,
                elev[i] as f32 + y_off,
                (p.y + rr.y) as f32,
            ));
            let v = match v_override {
                Some(vs) => vs[i],
                None => (cum[i] / v_tile.max(0.5)) as f32,
            };
            part.uv.push([u_ls[i], v]);
            part.uv.push([u_rs[i], v]);
        }
        for i in 0..pts.len() - 1 {
            // skip fully collapsed quads (wedged-out lane tails)
            if (off_l[i] - off_r[i]).abs() < 1e-4 && (off_l[i + 1] - off_r[i + 1]).abs() < 1e-4 {
                continue;
            }
            let a = (i * 2) as u32;
            let b = a + 1;
            let c = (i * 2 + 2) as u32;
            let d = (i * 2 + 3) as u32;
            part.indices.extend_from_slice(&[a, c, b, b, c, d]);
        }
        part
    }

    let mut ol = vec![0.0f64; n];
    let mut orr = vec![0.0f64; n];
    let mut ul = vec![0.0f32; n];
    let mut ur = vec![0.0f32; n];
    let const_u = |ul: &mut Vec<f32>, ur: &mut Vec<f32>, a: f64, b: f64| {
        for i in 0..n {
            ul[i] = a as f32;
            ur[i] = b as f32;
        }
    };

    /* ---- 4. pathways (sidewalk texture), behind the gutter apron ---- */
    for (pws, left) in [(&pw_l, true), (&pw_r, false)] {
        if !pws.iter().any(|&x| x > 0.0) {
            continue;
        }
        for i in 0..n {
            let g = if unpaved[i] || gut_w[i] <= 1e-4 { 0.0 } else { gut_w[i] };
            if left {
                ol[i] = -half[i] - g - pws[i];
                orr[i] = -half[i] - g;
            } else {
                ol[i] = half[i] + g;
                orr[i] = half[i] + g + pws[i];
            }
        }
        const_u(&mut ul, &mut ur, 0.0, 1.0);
        parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.01, SIDEWALK_TINT, TEX_SIDEWALK, &ul, &ur, 2.0, None));
    }

    /* ---- 5. low-quality mode: flat dynamic strips */
    if !q.markings {
        for i in 0..n {
            ol[i] = -half[i];
            orr[i] = half[i];
        }
        const_u(&mut ul, &mut ur, 0.0, 1.0);
        parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.02, rgb(ASPHALT, 1.0), -1, &ul, &ur, v_tile, None));
        if dirt {
            for i in 0..n {
                ol[i] = -half[i];
                orr[i] = lane_span_l(i);
            }
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.025, rgb(ROUGH_FLAT, 1.0), -1, &ul, &ur, v_dirt, None));
            for i in 0..n {
                ol[i] = lane_span_r(i);
                orr[i] = half[i];
            }
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.025, rgb(ROUGH_FLAT, 1.0), -1, &ul, &ur, v_dirt, None));
        }
        if m_half.iter().any(|&x| x > 0.0) {
            for i in 0..n {
                ol[i] = -m_half[i];
                orr[i] = m_half[i];
            }
            let mc = if dirt { rgb(CEMENT_FLAT, 1.0) } else { rgb(CONCRETE, 1.0) };
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.05, mc, -1, &ul, &ur, 4.0, None));
        }
        return parts;
    }

    /* ---- 6. shoulders (trim col 0; rough gravel when dirt) */
    for (shv, left) in [(&sh_l, true), (&sh_r, false)] {
        if !shv.iter().any(|&x| x > 0.0) {
            continue;
        }
        for i in 0..n {
            if left {
                ol[i] = -half[i];
                orr[i] = lane_span_l(i);
            } else {
                ol[i] = lane_span_r(i);
                orr[i] = half[i];
            }
        }
        if dirt {
            const_u(&mut ul, &mut ur, 0.0, 1.0);
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.02, ROUGH_TINT, TEX_ROUGH, &ul, &ur, v_dirt, None));
        } else {
            const_u(
                &mut ul,
                &mut ur,
                COL_SHOULDER as f64 * UV_COL,
                (COL_SHOULDER + 1) as f64 * UV_COL,
            );
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.02, ROAD_TINT, TEX_ROAD_TRIM, &ul, &ur, v_tile, Some(&v_snap)));
        }
    }

    /* ---- 7. lane strips (trim-sheet columns, taper-aware) ----
       Strips are emitted PER SEGMENT with layouts anchored at the two
       endpoint points: surviving lanes lerp their width, the extra
       outermost lane(s) wedge to zero (B_j(t) = lerp(j·w_a/n_a,
       j·w_b/n_b, t) from the median edge; the wedge strip spans the
       remaining outer band). Columns count from the median outward. */
    let mut sample_range_end = vec![0usize; seg_count];
    for (i, &k) in seg_of.iter().enumerate() {
        sample_range_end[k] = i + 1;
    }

    if dirt {
        // unmarked mud over each lane area (U continuous across both sides)
        for (wl, left) in [(&w_lft, true), (&w_rgt, false)] {
            if !wl.iter().any(|&x| x > 1e-4) {
                continue;
            }
            for i in 0..n {
                let tot = (w_lft[i] + w_rgt[i]).max(1e-6);
                if left {
                    ol[i] = lane_span_l(i);
                    orr[i] = -m_half[i];
                    ul[i] = 0.0;
                    ur[i] = (w_lft[i] / tot) as f32;
                } else {
                    ol[i] = m_half[i];
                    orr[i] = lane_span_r(i);
                    ul[i] = (w_lft[i] / tot) as f32;
                    ur[i] = 1.0;
                }
            }
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.02, MUD_TINT, TEX_MUD, &ul, &ur, v_dirt, None));
        }
    } else {
        // helper: emit one side's lane strips for one segment
        #[allow(clippy::too_many_arguments)]
        // helper: emit one side's lane strips for one segment.
        // Layouts are anchored at the two endpoint points: strip boundaries
        // B(q,t) = q · lerp(w_a/n_a, w_b/n_b, t) from the median edge; the
        // extra outer lane (q == nmin when counts differ) spans the whole
        // remaining band and wedges to zero at the far end. Surviving lanes
        // keep their inner edge pinned at the median, so the double-yellow
        // stays centered through the taper.
        #[allow(clippy::too_many_arguments)]
        fn emit_side(
            seg_range: (usize, usize),
            seg_t: &[f64],
            m_half: &[f64],
            elev: &[f64],
            n_a: usize,
            n_b: usize,
            w_a: f64,
            w_b: f64,
            left: bool,
            oneway_seg: bool,
            pts: &[Vec2],
            per: &[Vec2],
            cum: &[f64],
            v_snap: &[f32],
            parts: &mut Vec<MeshPart>,
        ) {
            let nmax = n_a.max(n_b);
            let nmin = n_a.min(n_b);
            if nmax == 0 {
                return;
            }
            let lw_a = w_a / n_a.max(1) as f64;
            let lw_b = w_b / n_b.max(1) as f64;
            let mut ol = vec![0.0f64; pts.len()];
            let mut orr = vec![0.0f64; pts.len()];
            let mut ul = vec![0.0f32; pts.len()];
            let mut ur = vec![0.0f32; pts.len()];
            for q in 0..nmax {
                let wedge = q == nmin && nmax > nmin;
                for i in seg_range.0..seg_range.1 {
                    let t = seg_t[i];
                    // GRG eases the cross-section interpolation
                    // (`ease(offset, -2.0)`) so each RoadPoint's layout is
                    // pinned exactly at the segment ends.
                    let te = crate::curve::ease_neg2(t);
                    let lw = lw_a + (lw_b - lw_a) * te;
                    let wband = w_a + (w_b - w_a) * te;
                    let (bin, bout) = if wedge {
                        // band inner edge lerps from nmin·lw_a to nmin·lw_b,
                        // outer edge is the full band width (→ zero wedge)
                        ((nmin as f64) * lw, wband)
                    } else {
                        (q as f64 * lw, (q + 1) as f64 * lw)
                    };
                    if left {
                        let inner = -m_half[i];
                        ol[i] = inner - bout;
                        orr[i] = inner - bin;
                    } else {
                        let inner = m_half[i];
                        ol[i] = inner + bin;
                        orr[i] = inner + bout;
                    }
                }
                let col = if left {
                    if nmax == 1 {
                        COL_TWO_WAY
                    } else if q == 0 {
                        COL_FAST // innermost: double-yellow at the median seam
                    } else if q == nmax - 1 {
                        COL_SLOW
                    } else {
                        COL_MIDDLE
                    }
                } else if oneway_seg {
                    if q == nmax - 1 { COL_SLOW } else { COL_MIDDLE }
                } else if q == 0 {
                    COL_TWO_WAY
                } else if q == nmax - 1 {
                    COL_SLOW
                } else {
                    COL_MIDDLE
                };
                let (ua, ub) = if oneway_seg && !left && q == nmax - 1 {
                    // one-way outer-right flips so the solid white sits on the edge
                    ((col as f64 + 1.0) * UV_COL, col as f64 * UV_COL)
                } else if left {
                    // REVERSE lanes mirror the column (GRG swap rule)
                    ((col as f64 + 1.0) * UV_COL, col as f64 * UV_COL)
                } else {
                    (col as f64 * UV_COL, (col as f64 + 1.0) * UV_COL)
                };
                for j in 0..pts.len() {
                    ul[j] = ua as f32;
                    ur[j] = ub as f32;
                }
                parts.push(strip_dyn(
                    pts, per, cum, elev, &ol, &orr, 0.02, ROAD_TINT, TEX_ROAD_TRIM, &ul, &ur,
                    14.0, Some(v_snap),
                ));
            }
        }

        for k in 0..seg_count {
            let a = &curve.points[k];
            let b = &curve.points[(k + 1) % n_points];
            let (la, ra) = split(a.lanes.max(1) as i32, a.oneway);
            let (lb, rb) = split(b.lanes.max(1) as i32, b.oneway);
            let end = if k + 1 < seg_count { seg_start[k + 1] + 1 } else { n };
            let range = (seg_start[k], end.min(n));
            // real side band widths at the segment endpoints (lane area only:
            // half paved minus own shoulder minus half median — matches the
            // per-sample w_lft/w_rgt so strip edges land exactly on the
            // shoulder line through the whole taper; one-way puts every lane
            // on the right band)
            let w_a_l = la as f64 * a.lane_width + (a.shoulder_r - a.shoulder_l) / 2.0;
            let w_b_l = lb as f64 * b.lane_width + (b.shoulder_r - b.shoulder_l) / 2.0;
            let w_a_r = ra as f64 * a.lane_width + (a.shoulder_l - a.shoulder_r) / 2.0;
            let w_b_r = rb as f64 * b.lane_width + (b.shoulder_l - b.shoulder_r) / 2.0;
            if !a.oneway && !b.oneway {
                emit_side(range, &seg_t, &m_half, &elev, la, lb, w_a_l, w_b_l, true, false, &pts, &per, &cum, &v_snap, &mut parts);
            }
            emit_side(range, &seg_t, &m_half, &elev, ra, rb, w_a_r, w_b_r, false, a.oneway, &pts, &per, &cum, &v_snap, &mut parts);
        }
    }

    /* ---- 8. physical median: BRT cement (>= 3 m) or painted median */
    if m_half.iter().any(|&x| x > 0.0) {
        let max_med = m_half.iter().fold(0.0f64, |m, &x| m.max(x * 2.0));
        for i in 0..n {
            ol[i] = -m_half[i];
            orr[i] = m_half[i];
        }
        const_u(&mut ul, &mut ur, 0.0, 1.0);
        let mtex = if dirt || max_med >= BRT_MEDIAN_M { TEX_CEMENT } else { TEX_MEDIAN };
        parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.05, CONCRETE_TINT, mtex, &ul, &ur, 4.0, None));
        if max_med >= BRT_MEDIAN_M {
            let tw = TACTILE_W;
            for i in 0..n {
                ol[i] = -m_half[i];
                orr[i] = -m_half[i] + tw.min(m_half[i].max(1e-6));
            }
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.06, TACTILE_TINT, TEX_TACTILE, &ul, &ur, 0.8, None));
            for i in 0..n {
                ol[i] = m_half[i] - tw.min(m_half[i].max(1e-6));
                orr[i] = m_half[i];
            }
            parts.push(strip_dyn(&pts, &per, &cum, &elev, &ol, &orr, 0.06, TACTILE_TINT, TEX_TACTILE, &ul, &ur, 0.8, None));
        }
    }

    parts
}

/* ---- flyover piers ------------------------------------------------- */

/// Concrete piers under elevated (bridge / flyover) stretches — JICA
/// §5.3.5 grade separation: a deck on 4 % approaches needs visible
/// supports. Emits a box column every ~26 m of arc wherever the road
/// surface sits > 2.5 m above the ground plane (COL_PLAIN trim column).
pub fn pier_parts(curve: &RoadCurve) -> Vec<MeshPart> {
    let mut parts = Vec::new();
    if !curve.has_elevated() {
        return parts;
    }
    let pts = curve.centerline(6.0);
    if pts.len() < 2 {
        return parts;
    }
    let pier_half_w = (curve.paved_width() * 0.22).clamp(0.8, 2.6);
    const PIER_DEPTH: f64 = 2.2;
    const SPACING: f64 = 26.0;
    let mut since_last = SPACING; // force a pier at the first elevated sample
    for i in 0..pts.len() - 1 {
        let a = pts[i];
        let b = pts[i + 1];
        let seg = ((b.x - a.x) as f64).hypot((b.z - a.z) as f64).max(1e-6);
        since_last += seg;
        let deck_y = a.y as f64;
        if deck_y <= 2.5 || since_last < SPACING {
            continue;
        }
        since_last = 0.0;
        // pier top just under the deck underside
        let top = deck_y - 0.35;
        if top < 1.0 {
            continue;
        }
        // perpendicular of the centerline direction
        let dx = (b.x - a.x) as f64 / seg;
        let dz = (b.z - a.z) as f64 / seg;
        let (px, pz) = (-dz, dx);
        let (cx, cz) = (a.x as f64, a.z as f64);
        let u0 = (COL_PLAIN as f64 * UV_COL) as f32;
        let u1 = ((COL_PLAIN + 1) as f64 * UV_COL) as f32;
        let mut part = MeshPart::new(CONCRETE_TINT);
        part.tex = TEX_CEMENT;
        // 4 corners of the box footprint
        let corners = [
            (cx + px * pier_half_w + 0.0, cz + pz * pier_half_w),
            (cx - px * pier_half_w + 0.0, cz - pz * pier_half_w),
        ];
        // build the 4 side quads: two faces across the width, two across depth
        let faces = [
            // across-width faces (offset ± depth/2 along the road dir)
            (
                (cx + px * pier_half_w - dx * PIER_DEPTH / 2.0, cz + pz * pier_half_w - dz * PIER_DEPTH / 2.0),
                (cx + px * pier_half_w + dx * PIER_DEPTH / 2.0, cz + pz * pier_half_w + dz * PIER_DEPTH / 2.0),
            ),
            (
                (cx - px * pier_half_w - dx * PIER_DEPTH / 2.0, cz - pz * pier_half_w - dz * PIER_DEPTH / 2.0),
                (cx - px * pier_half_w + dx * PIER_DEPTH / 2.0, cz - pz * pier_half_w + dz * PIER_DEPTH / 2.0),
            ),
            // across-depth faces
            (
                (cx + px * pier_half_w - dx * PIER_DEPTH / 2.0, cz + pz * pier_half_w - dz * PIER_DEPTH / 2.0),
                (cx - px * pier_half_w - dx * PIER_DEPTH / 2.0, cz - pz * pier_half_w - dz * PIER_DEPTH / 2.0),
            ),
            (
                (cx + px * pier_half_w + dx * PIER_DEPTH / 2.0, cz + pz * pier_half_w + dz * PIER_DEPTH / 2.0),
                (cx - px * pier_half_w + dx * PIER_DEPTH / 2.0, cz - pz * pier_half_w + dz * PIER_DEPTH / 2.0),
            ),
        ];
        for ((x0, z0), (x1, z1)) in faces {
            let base = part.positions.len() as u32;
            part.positions.push(Vec3::new(x0 as f32, 0.0, z0 as f32));
            part.positions.push(Vec3::new(x1 as f32, 0.0, z1 as f32));
            part.positions.push(Vec3::new(x1 as f32, top as f32, z1 as f32));
            part.positions.push(Vec3::new(x0 as f32, top as f32, z0 as f32));
            part.uv.push([u0, 0.0]);
            part.uv.push([u1, 0.0]);
            part.uv.push([u1, (top / 3.0) as f32]);
            part.uv.push([u0, (top / 3.0) as f32]);
            for idx in [0u32, 1, 2, 0, 2, 3] {
                part.indices.push(base + idx);
            }
        }
        let _ = corners;
        parts.push(part);
    }
    parts
}
