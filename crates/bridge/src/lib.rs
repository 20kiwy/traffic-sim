//! bridge — the native bridge system for traffic-sim.
//!
//! Specs are derived from the OSM provider tags (`bridge=viaduct|bridge`,
//! `layer`, `bridge:structure`), and the geometry is real: a swept deck box
//! (asphalt top, cement/concrete box sides), side railings, pier columns with
//! caps every N meters down to the sampled ground, abutments at both ends,
//! and the computed clearance to whatever passes underneath — so the
//! intersection below and the road on top are both visible, like the
//! John Kijazi Flyover in Ubungo.

use gdmath::Vec3;
use roads::mesh3d::MeshPart;

/// surface colors (tints multiplied over the texture slots)
const CEMENT_TINT: [f32; 4] = [1.30, 1.28, 1.24, 1.0];
const ASPHALT_TINT: [f32; 4] = [1.45, 1.45, 1.47, 1.0];
const RAIL_TINT: [f32; 4] = [1.15, 1.14, 1.10, 1.0];
const PIER_TINT: [f32; 4] = [1.25, 1.23, 1.19, 1.0];
/// painted-line tint for deck markings (bright white paint, flat)
const MARK_TINT: [f32; 4] = [2.05, 2.05, 2.0, 1.0];

/// texture slots (roadkit::textures)
const TEX_ASPHALT: i32 = 4;
const TEX_CEMENT: i32 = 5;
const TEX_SIDEWALK: i32 = 2;

/// Bridge kind from the OSM `bridge` tag.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BridgeKind {
    Viaduct,
    Beam,
    Arch,
    Movable,
    Causeway,
}

impl BridgeKind {
    pub fn from_tag(t: &str) -> Self {
        match t {
            "viaduct" => BridgeKind::Viaduct,
            "arch" => BridgeKind::Arch,
            "movable" | "bascule" | "swing" => BridgeKind::Movable,
            "causeway" => BridgeKind::Causeway,
            _ => BridgeKind::Beam,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            BridgeKind::Viaduct => "viaduct",
            BridgeKind::Beam => "beam bridge",
            BridgeKind::Arch => "arch bridge",
            BridgeKind::Movable => "movable bridge",
            BridgeKind::Causeway => "causeway",
        }
    }
    /// construction means: pier spacing in meters
    pub fn pier_every(self) -> f64 {
        match self {
            BridgeKind::Viaduct => 28.0,
            BridgeKind::Beam => 34.0,
            BridgeKind::Arch => 60.0,
            BridgeKind::Movable => 45.0,
            BridgeKind::Causeway => 80.0,
        }
    }
    /// default clearance over the lower road (meters)
    pub fn default_clearance(self) -> f64 {
        match self {
            BridgeKind::Viaduct => 6.0,
            BridgeKind::Beam => 5.2,
            BridgeKind::Arch => 7.5,
            BridgeKind::Movable => 9.0,
            BridgeKind::Causeway => 1.5,
        }
    }
}

/// A bridge build spec.
#[derive(Clone, Debug)]
pub struct BridgeSpec {
    pub name: String,
    pub kind: BridgeKind,
    /// OSM layer of the deck (1 = first level above ground, 2 = above that)
    pub layer: i32,
    /// deck width in meters (from lanes x lane_width + median + shoulders)
    pub deck_width_m: f64,
    /// deck slab thickness
    pub deck_th_m: f64,
    /// railing height
    pub rail_h_m: f64,
    /// clearance over the ground/under-road (meters); 0 = use kind default
    pub clearance_m: f64,
    /// deck surface surface-type constant (roads::surface) — cement for
    /// concrete viaduct decks, e.g. the BRT Morogoro Road corridor
    pub surface: u8,
    /// painted median width on the deck (0 = single carriageway, dashed
    /// center line); BRT corridors carry a 3.0 m median
    pub median_m: f64,
}

impl BridgeSpec {
    pub fn from_tags(name: &str, bridge: &str, layer: i32, lanes: usize, has_brt: bool) -> Self {
        let kind = BridgeKind::from_tag(bridge);
        // deck width: lanes * 3.5 + brt median 3.0 + 2x0.5 shoulders, min 9
        let mut w = (lanes.max(2) as f64) * 3.5 + 1.0;
        if has_brt {
            w += 3.0;
        }
        BridgeSpec {
            name: name.to_string(),
            kind,
            layer,
            deck_width_m: w.max(9.0),
            deck_th_m: 0.9,
            rail_h_m: 1.1,
            clearance_m: 0.0,
            surface: if has_brt || kind == BridgeKind::Viaduct {
                roads::surface::CEMENT
            } else {
                roads::surface::ASPHALT
            },
            median_m: if has_brt { 3.0 } else { 0.0 },
        }
    }

    /// The John Kijazi Flyover — Nelson Mandela Road, layer 2, Ubungo.
    pub fn kijazi_flyover() -> Self {
        let mut s = Self::from_tags("John Kijazi Flyover", "viaduct", 2, 4, false);
        s.clearance_m = 6.2;
        s
    }

    /// The John Kijazi Bridge — Morogoro Road (T1 trunk) with the BRT
    /// `lanes:psv` corridor, layer 1, cement deck.
    pub fn kijazi_bridge() -> Self {
        let mut s = Self::from_tags("John Kijazi Bridge", "viaduct", 1, 3, true);
        s.clearance_m = 5.6;
        s
    }
}

/// Input points: (x, ground_y, z) in local meters, along the deck centerline.
pub struct BridgeInput {
    pub pts: Vec<(f64, f64, f64)>,
}

/// The built bridge geometry + measurements.
pub struct BridgeBuild {
    pub parts: Vec<MeshPart>,
    pub name: String,
    pub kind: &'static str,
    pub length_m: f64,
    pub deck_y: f64,
    /// computed clearance over the highest ground crossed
    pub clear_below_m: f64,
    pub pier_count: usize,
}

fn quad(p: &mut MeshPart, a: Vec3, b: Vec3, c: Vec3, d: Vec3, u0: f32, u1: f32, v0: f32, v1: f32) {
    let base = p.positions.len() as u32;
    p.positions.extend_from_slice(&[a, b, c, d]);
    p.uv.extend_from_slice(&[[u0, v0], [u1, v0], [u1, v1], [u0, v1]]);
    p.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// Build the full bridge over `input`.
pub fn build_bridge(input: &BridgeInput, spec: &BridgeSpec) -> BridgeBuild {
    let pts = &input.pts;
    let mut parts: Vec<MeshPart> = Vec::new();
    let mut deck_top = MeshPart::new(ASPHALT_TINT);
    deck_top.tex = TEX_ASPHALT;
    let mut deck_sides = MeshPart::new(CEMENT_TINT);
    deck_sides.tex = TEX_CEMENT;
    let mut rails = MeshPart::new(RAIL_TINT);
    rails.tex = TEX_SIDEWALK;
    let mut piers = MeshPart::new(PIER_TINT);
    piers.tex = TEX_CEMENT;
    // painted lane markings on the deck top — flat white paint, geometric
    // dashes (tex -1 = plain vertex color, always crisp)
    let mut marks = MeshPart::new(MARK_TINT);
    marks.tex = -1;

    let n = pts.len();
    let half = spec.deck_width_m / 2.0;
    let th = spec.deck_th_m;

    // ground reference along the span
    let max_ground = pts.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max);
    let min_ground = pts.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
    let clearance = if spec.clearance_m > 0.0 {
        spec.clearance_m
    } else {
        spec.kind.default_clearance()
    };
    let deck_y = max_ground + clearance;
    let top = deck_y;
    let bot = deck_y - th;

    // right vector per segment for the deck cross-section
    let right = |i: usize| -> [f64; 2] {
        let a = pts[i.min(n - 1)];
        let b = pts[(i + 1).min(n - 1)];
        let dx = b.0 - a.0;
        let dz = b.2 - a.2;
        let l = dx.hypot(dz).max(1e-6);
        [dz / l, -dx / l] // (dz, -dx) = right-hand normal in xz
    };

    let mut length = 0.0f64;
    let mut cum = vec![0.0f64; n];
    for i in 1..n {
        let a = pts[i - 1];
        let b = pts[i];
        length += (b.0 - a.0).hypot(b.2 - a.2);
        cum[i] = length;
    }
    let uv_h = 8.0f64; // meters per texture repeat along the deck

    // deck ribbon: top face + bottom face + two side skirts + railings
    for i in 0..n {
        let p = pts[i];
        let r = right(i);
        let cx = p.0;
        let cz = p.2;
        // top surface
        let a = Vec3::new((cx - r[0] * half) as f32, top as f32, (cz - r[1] * half) as f32);
        let b = Vec3::new((cx + r[0] * half) as f32, top as f32, (cz + r[1] * half) as f32);
        if i + 1 < n {
            let p2 = pts[i + 1];
            let r2 = right(i + 1);
            let c = Vec3::new(
                (p2.0 + r2[0] * half) as f32,
                top as f32,
                (p2.2 + r2[1] * half) as f32,
            );
            let d = Vec3::new(
                (p2.0 - r2[0] * half) as f32,
                top as f32,
                (p2.2 - r2[1] * half) as f32,
            );
            let v0 = (cum[i] / uv_h) as f32;
            let v1 = (cum[i + 1] / uv_h) as f32;
            quad(&mut deck_top, a, b, c, d, 0.0, 1.0, v0, v1);

            // painted markings, 2 cm above the deck top: solid edge lines at
            // ±(half − 0.55) and a dashed center line (or per-carriageway
            // dashed lines flanking the BRT median). Dashes are emitted
            // geometrically (1.6 m on / 3.2 m period) in arc-length space.
            {
                let lift = 0.02f64;
                let r2 = right(i + 1);
                let p2 = pts[i + 1];
                let (s0, s1) = (cum[i], cum[i + 1]);
                let seg_len = (s1 - s0).max(1e-6);
                let at = |t: f64, off: f64| {
                    let rl = r[0] + (r2[0] - r[0]) * t;
                    let rz = r[1] + (r2[1] - r[1]) * t;
                    Vec3::new(
                        (p.0 + (p2.0 - p.0) * t + rl * off) as f32,
                        (top + lift) as f32,
                        (p.2 + (p2.2 - p.2) * t + rz * off) as f32,
                    )
                };
                let mut line = |o: f64, w: f64, dashed: bool| {
                    let (oa, ob) = (o - w * 0.5, o + w * 0.5);
                    let ranges: Vec<(f64, f64)> = if dashed {
                        let (period, on) = (3.2f64, 1.6f64);
                        let mut v = Vec::new();
                        let mut k = (s0 / period).floor();
                        while k * period < s1 {
                            let a = (k * period).max(s0);
                            let b = (k * period + on).min(s1);
                            if b > a {
                                v.push((a, b));
                            }
                            k += 1.0;
                        }
                        v
                    } else {
                        vec![(s0, s1)]
                    };
                    for (a, b) in ranges {
                        let t0 = ((a - s0) / seg_len).clamp(0.0, 1.0);
                        let t1 = ((b - s0) / seg_len).clamp(0.0, 1.0);
                        quad(
                            &mut marks,
                            at(t0, oa),
                            at(t0, ob),
                            at(t1, ob),
                            at(t1, oa),
                            0.0,
                            1.0,
                            0.0,
                            1.0,
                        );
                    }
                };
                line(-(half - 0.55), 0.15, false);
                line(half - 0.55, 0.15, false);
                if spec.median_m > 0.01 {
                    let lane_c = spec.median_m * 0.5 + 1.75;
                    line(-lane_c, 0.15, true);
                    line(lane_c, 0.15, true);
                } else {
                    line(0.0, 0.18, true);
                }
            }

            // bottom face (reversed)
            let a2 = Vec3::new(a.x, bot as f32, a.z);
            let b2 = Vec3::new(b.x, bot as f32, b.z);
            let c2 = Vec3::new(c.x, bot as f32, c.z);
            let d2 = Vec3::new(d.x, bot as f32, d.z);
            quad(&mut deck_sides, d2, c2, b2, a2, 0.0, 1.0, v0, v1);

            // side skirts (box girder look)
            let s0a = a;
            let s0b = b;
            let s1a = Vec3::new(a2.x, bot as f32, a2.z);
            let s1b = Vec3::new(b2.x, bot as f32, b2.z);
            quad(&mut deck_sides, s0a, s1a, s1b, s0b, 0.0, 0.125, v0, v1); // left
            let t0a = b;
            let t0b = a;
            let t1a = Vec3::new(b2.x, bot as f32, b2.z);
            let t1b = Vec3::new(a2.x, bot as f32, a2.z);
            quad(&mut deck_sides, t0a, t1a, t1b, t0b, 0.125, 0.25, v0, v1); // right

            // railings: small walls on both edges
            let rw = 0.18f64;
            for (sgn, mesh) in [(-1.0, 0), (1.0, 1)] {
                let rx = r[0] * sgn * (half - rw * 0.5);
                let rz = r[1] * sgn * (half - rw * 0.5);
                let ba = Vec3::new((cx + rx) as f32, top as f32, (cz + rz) as f32);
                let ta = Vec3::new((cx + rx) as f32, (top + spec.rail_h_m) as f32, (cz + rz) as f32);
                let r2x = r2[0] * sgn * (half - rw * 0.5);
                let r2z = r2[1] * sgn * (half - rw * 0.5);
                let bb = Vec3::new(
                    (p2.0 + r2x) as f32,
                    top as f32,
                    (p2.2 + r2z) as f32,
                );
                let tb = Vec3::new(
                    (p2.0 + r2x) as f32,
                    (top + spec.rail_h_m) as f32,
                    (p2.2 + r2z) as f32,
                );
                let target = if mesh == 0 { &mut rails } else { &mut rails };
                quad(target, ba, ta, tb, bb, 0.0, 0.06, v0, v1);
            }
        }
    }

    // piers: 12-gon tapered columns every pier_every meters of arc length
    let every = spec.kind.pier_every();
    let pier_r = (1.1 + spec.deck_width_m * 0.045).min(2.2);
    let mut next_at = every * 0.5;
    let mut pier_count = 0usize;
    while next_at < length - every * 0.35 {
        // find point at arc s
        let mut i = 1;
        while i < n - 1 && cum[i] < next_at {
            i += 1;
        }
        let t = if cum[i] > cum[i - 1] {
            (next_at - cum[i - 1]) / (cum[i] - cum[i - 1])
        } else {
            0.0
        };
        let a = pts[i - 1];
        let b = pts[i];
        let px = a.0 + (b.0 - a.0) * t;
        let pz = a.2 + (b.2 - a.2) * t;
        let gy = a.1 + (b.1 - a.1) * t;
        let r = right(i);
        let cap_top = bot - 0.15;
        let cap_h = 0.7;
        // pier cap: wider box
        let cap_w = pier_r * 2.4;
        let cx = px;
        let cz = pz;
        let a = Vec3::new((cx - r[0] * cap_w) as f32, (cap_top) as f32, (cz - r[1] * cap_w) as f32);
        let bb = Vec3::new((cx + r[0] * cap_w) as f32, (cap_top) as f32, (cz + r[1] * cap_w) as f32);
        let c = Vec3::new(
            (cx + r[0] * cap_w) as f32,
            (cap_top - cap_h) as f32,
            (cz + r[1] * cap_w) as f32,
        );
        let d = Vec3::new(
            (cx - r[0] * cap_w) as f32,
            (cap_top - cap_h) as f32,
            (cz - r[1] * cap_w) as f32,
        );
        quad(&mut piers, a, bb, c, d, 0.0, 0.2, 0.0, 0.1);

        // column: 12-gon prism from ground to cap bottom
        let sides = 12;
        let base = piers.positions.len() as u32;
        let col_top = cap_top - cap_h;
        for k in 0..sides {
            let ang = (k as f64) * std::f64::consts::TAU / sides as f64;
            let ox = ang.cos() * pier_r;
            let oz = ang.sin() * pier_r;
            let uy = (ang + std::f64::consts::TAU / sides as f64).cos() * pier_r;
            let uz = (ang + std::f64::consts::TAU / sides as f64).sin() * pier_r;
            piers.positions.push(Vec3::new((cx + ox) as f32, col_top as f32, (cz + oz) as f32));
            piers.positions.push(Vec3::new((cx + uy) as f32, col_top as f32, (cz + uz) as f32));
            piers.positions.push(Vec3::new((cx + uy) as f32, gy as f32, (cz + uz) as f32));
            piers.positions.push(Vec3::new((cx + ox) as f32, gy as f32, (cz + oz) as f32));
            let v0 = k as f32 / sides as f32;
            let v1 = (k + 1) as f32 / sides as f32;
            piers.uv.extend_from_slice(&[
                [v0 * 0.25, 0.0],
                [v1 * 0.25, 0.0],
                [v1 * 0.25, 1.0],
                [v0 * 0.25, 1.0],
            ]);
            let q = base + (k * 4) as u32;
            piers.indices
                .extend_from_slice(&[q, q + 1, q + 2, q, q + 2, q + 3]);
        }
        pier_count += 1;
        next_at += every;
    }

    // abutments at both ends: blocks from ground to deck at the ends
    for (i, sgn) in [(0usize, -1.0f64), (n - 1, 1.0f64)] {
        let p = pts[i];
        let r = right(i);
        let gy = p.1 - 0.5;
        let aw = half + 0.6;
        let ad = 1.6;
        let dir = right(i);
        let dx = dir[0] * ad * -sgn;
        let dz = dir[1] * ad * -sgn;
        let y0 = deck_y - th;
        let corners = [
            Vec3::new((p.0 - r[0] * aw) as f32, y0 as f32, (p.2 - r[1] * aw) as f32),
            Vec3::new((p.0 + r[0] * aw) as f32, y0 as f32, (p.2 + r[1] * aw) as f32),
            Vec3::new((p.0 + r[0] * aw + dx) as f32, y0 as f32, (p.2 + r[1] * aw + dz) as f32),
            Vec3::new((p.0 - r[0] * aw + dx) as f32, y0 as f32, (p.2 - r[1] * aw + dz) as f32),
        ];
        for k in 0..4 {
            let k2 = (k + 1) % 4;
            let a = corners[k];
            let b = corners[k2];
            let a2 = Vec3::new(a.x, gy as f32, a.z);
            let b2 = Vec3::new(b.x, gy as f32, b.z);
            quad(&mut piers, a, b, b2, a2, 0.0, 0.2, 0.0, 1.0);
        }
    }

    parts.push(deck_top);
    parts.push(deck_sides);
    parts.push(rails);
    parts.push(piers);
    parts.push(marks);

    BridgeBuild {
        parts,
        name: spec.name.clone(),
        kind: spec.kind.name(),
        length_m: length,
        deck_y,
        clear_below_m: deck_y - th - max_ground.max(min_ground).max(0.0),
        pier_count,
    }
}

/// Convenience: build from a BridgeFeature's lat/lon nodes + DEM sampler.
pub fn build_from_feature(
    nodes_ll: &[[f64; 2]],
    spec: &BridgeSpec,
    ground_at: impl Fn(f64, f64) -> f64,
) -> BridgeBuild {
    let pts: Vec<(f64, f64, f64)> = nodes_ll
        .iter()
        .map(|&p| {
            let (lat, lon) = (p[0], p[1]);
            let (x, z) = engine_terrain_latlon_to_local(lat, lon);
            (x, ground_at(lat, lon), z)
        })
        .collect();
    build_bridge(&BridgeInput { pts }, spec)
}

fn engine_terrain_latlon_to_local(lat: f64, lon: f64) -> (f64, f64) {
    engine::terrain::latlon_to_local(lat, lon)
}
