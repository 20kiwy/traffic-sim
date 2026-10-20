//! Procedural city blocks — the RoadArchitect-style roadside richness for
//! the DSM scenarios (R11): multi-storey buildings assembled from floor
//! slabs, windowed facades, parapets, rooftop boxes ("ceilings"), water
//! tanks and ground-floor formwork columns (pilotis) for commercial styles.
//!
//! Placement is LOGIC-RESTRICTED (docs/ROADARCHITECT.md §9): candidate
//! blocks on a jittered grid are rejected when they overlap a road
//! corridor (distance to any RoadPoint < paved/2 + setback) or an already
//! accepted building. All geometry is world-space; `scene.rs` recenters it
//! with the rest of the mesh parts.

use crate::textures::{TEX_RA_BRICK, TEX_RA_CONCRETE, TEX_WINDOWS};
use gdmath::vec3::Vec3;
use roads::mesh3d::MeshPart;
use roads::model::RoadCurve;

/// Deterministic LCG (same stream discipline as props.rs).
struct Lcg(u64);
impl Lcg {
    fn next_f64(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((self.0 >> 33) as f64) / ((1u64 << 31) as f64)
    }
    fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.next_f64()
    }
}

const FLOOR_H: f32 = 3.2;
const SLAB_T: f32 = 0.22;
const SLAB_OVER: f32 = 0.14;
const PARAPET_H: f32 = 0.55;
const WALL_INSET: f32 = 0.0;

fn quad(part: &mut MeshPart, p: [Vec3; 4], uv: [[f32; 2]; 4]) {
    let base = part.positions.len() as u32;
    part.positions.extend_from_slice(&p);
    part.uv.extend_from_slice(&uv);
    part.indices
        .extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
}

/// One procedural building at (x, z), footprint w×d, `floors` storeys,
/// rotated by `yaw` around its center. Commercial adds ground-floor
/// formwork columns + recessed glass; every style gets floor slabs, a
/// rooftop slab ("ceiling"), parapet, stair box and a rooftop water tank.
pub fn building_parts(
    x: f64,
    z: f64,
    w: f64,
    d: f64,
    floors: usize,
    yaw: f64,
    seed: u64,
) -> Vec<MeshPart> {
    let mut rng = Lcg(seed);
    let (mut walls, mut slabs, mut accents) = (
        MeshPart::new([1.02, 0.99, 0.94, 1.0]), // warm plaster
        MeshPart::new([0.88, 0.87, 0.85, 1.0]), // mid-gray concrete
        MeshPart::new([0.9, 0.72, 0.55, 1.0]),  // terracotta brick band
    );
    walls.tex = TEX_WINDOWS;
    slabs.tex = TEX_RA_CONCRETE;
    accents.tex = TEX_RA_BRICK;
    let floors = floors.clamp(1, 8);
    let hw = w as f32 / 2.0;
    let hd = d as f32 / 2.0;
    let (ca, sa) = (yaw.cos() as f32, yaw.sin() as f32);
    // local -> world (x, z rotated around the building center)
    let lw = |lx: f32, lz: f32| (x as f32 + lx * ca - lz * sa, z as f32 + lx * sa + lz * ca);
    let htotal = floors as f32 * FLOOR_H;

    // ---- plinth (slabs part) ----
    let pw = hw + 0.25;
    let pd = hd + 0.25;
    for (lx0, lz0, lx1, lz1) in [
        (-pw, -pd, pw, -pd),
        (pw, -pd, pw, pd),
        (pw, pd, -pw, pd),
        (-pw, pd, -pw, -pd),
    ] {
        let (ax, az) = lw(lx0, lz0);
        let (bx, bz) = lw(lx1, lz1);
        quad(
            &mut slabs,
            [
                Vec3::new(ax, 0.0, az),
                Vec3::new(bx, 0.0, bz),
                Vec3::new(bx, 0.25, bz),
                Vec3::new(ax, 0.25, az),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 0.06], [0.0, 0.06]],
        );
    }

    // ---- facades (windows, one tile per 4 m × one floor) ----
    let faces: [(f32, f32, f32, f32); 4] = [
        (-hw, -hd, hw, -hd), // south
        (hw, -hd, hw, hd),   // east
        (hw, hd, -hw, hd),   // north
        (-hw, hd, -hw, -hd), // west
    ];
    for f in 0..floors {
        let y0 = f as f32 * FLOOR_H + 0.22;
        let y1 = (f + 1) as f32 * FLOOR_H - SLAB_T;
        for (lx0, lz0, lx1, lz1) in faces {
            let (ax, az) = lw(lx0 - WALL_INSET * (lx0).signum(), lz0 - WALL_INSET * (lz0).signum());
            let (bx, bz) = lw(lx1 - WALL_INSET * (lx1).signum(), lz1 - WALL_INSET * (lz1).signum());
            let span = ((bx - ax).hypot(bz - az)).max(1.0);
            quad(
                &mut walls,
                [
                    Vec3::new(ax, y0, az),
                    Vec3::new(bx, y0, bz),
                    Vec3::new(bx, y1, bz),
                    Vec3::new(ax, y1, az),
                ],
                [
                    [0.0, f as f32],
                    [span / 4.0, f as f32],
                    [span / 4.0, f as f32 + 1.0],
                    [0.0, f as f32 + 1.0],
                ],
            );
        }
        // ---- floor slab band (the "slabs" the user asked for) ----
        let yb = (f + 1) as f32 * FLOOR_H;
        let bw = hw + SLAB_OVER;
        let bd = hd + SLAB_OVER;
        for (lx0, lz0, lx1, lz1) in [
            (-bw, -bd, bw, -bd),
            (bw, -bd, bw, bd),
            (bw, bd, -bw, bd),
            (-bw, bd, -bw, -bd),
        ] {
            let (ax, az) = lw(lx0, lz0);
            let (bx, bz) = lw(lx1, lz1);
            quad(
                &mut slabs,
                [
                    Vec3::new(ax, yb - SLAB_T, az),
                    Vec3::new(bx, yb - SLAB_T, bz),
                    Vec3::new(bx, yb, bz),
                    Vec3::new(ax, yb, az),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 0.05], [0.0, 0.05]],
            );
        }
    }

    // ---- roof slab + parapet + rooftop box (ceiling) + water tank ----
    {
        // top slab (ceiling underside)
        let (ax, az) = lw(-hw, -hd);
        let (bx, bz) = lw(hw, hd);
        quad(
            &mut slabs,
            [
                Vec3::new(ax, htotal, az),
                Vec3::new(bx, htotal, az),
                Vec3::new(bx, htotal, bz),
                Vec3::new(ax, htotal, bz),
            ],
            [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
        );
        // parapet ring
        for (lx0, lz0, lx1, lz1) in [
            (-hw - 0.1, -hd - 0.1, hw + 0.1, -hd - 0.1),
            (hw + 0.1, -hd - 0.1, hw + 0.1, hd + 0.1),
            (hw + 0.1, hd + 0.1, -hw - 0.1, hd + 0.1),
            (-hw - 0.1, hd + 0.1, -hw - 0.1, -hd - 0.1),
        ] {
            let (ax, az) = lw(lx0, lz0);
            let (bx, bz) = lw(lx1, lz1);
            quad(
                &mut slabs,
                [
                    Vec3::new(ax, htotal, az),
                    Vec3::new(bx, htotal, bz),
                    Vec3::new(bx, htotal + PARAPET_H, bz),
                    Vec3::new(ax, htotal + PARAPET_H, az),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 0.1], [0.0, 0.1]],
            );
        }
        // stair box
        let bw2 = (hw * 0.34).max(1.4);
        let bd2 = (hd * 0.34).max(1.2);
        let ox = hw * 0.4;
        for (lx0, lz0, lx1, lz1) in [
            (ox - bw2, -bd2, ox + bw2, -bd2),
            (ox + bw2, -bd2, ox + bw2, bd2),
            (ox + bw2, bd2, ox - bw2, bd2),
            (ox - bw2, bd2, ox - bw2, -bd2),
        ] {
            let (ax, az) = lw(lx0, lz0);
            let (bx, bz) = lw(lx1, lz1);
            quad(
                &mut walls,
                [
                    Vec3::new(ax, htotal, az),
                    Vec3::new(bx, htotal, bz),
                    Vec3::new(bx, htotal + 2.4, bz),
                    Vec3::new(ax, htotal + 2.4, az),
                ],
                [[0.0, 0.0], [1.0, 0.0], [1.0, 0.6], [0.0, 0.6]],
            );
        }
        // water tank (Dar rooftops always carry one) — 8-gon cylinder
        let tk_r = 0.85;
        let tk_h = 1.1;
        let t_base = len_acc(&mut accents.positions);
        let (tx, tz) = lw(-hw * 0.45, hd * 0.4);
        for s in 0..8 {
            let a0 = std::f32::consts::TAU * s as f32 / 8.0;
            let a1 = std::f32::consts::TAU * (s + 1) as f32 / 8.0;
            let base = accents.positions.len() as u32;
            for (a, y) in [(a0, 0.0), (a1, 0.0), (a1, tk_h), (a0, tk_h)] {
                accents
                    .positions
                    .push(Vec3::new(tx as f32 + a.cos() * tk_r, htotal + y, tz as f32 + a.sin() * tk_r));
            }
            accents
                .uv
                .extend_from_slice(&[[0.0, 0.0], [0.3, 0.0], [0.3, 0.3], [0.0, 0.3]]);
            accents.indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
        let _ = t_base;
    }

    // ---- commercial ground floor: formwork columns + recessed glass ----
    let mut parts = vec![walls, slabs, accents];
    if seed % 3 == 0 && floors >= 2 {
        let mut cols = MeshPart::new([0.92, 0.91, 0.89, 1.0]);
        cols.tex = TEX_RA_CONCRETE;
        let n = ((w as f32 / 5.0).round() as usize).clamp(3, 6);
        for i in 0..=n {
            let lx = -hw + 2.0 * hw * (i as f32) / n as f32;
            for lz in [-hd + 0.3, hd - 0.3] {
                let (px, pz) = lw(lx, lz);
                let base = cols.positions.len() as u32;
                let r = 0.22f32;
                for s in 0..4 {
                    let a0 = std::f32::consts::FRAC_PI_2 * s as f32;
                    let a1 = std::f32::consts::FRAC_PI_2 * (s + 1) as f32;
                    let b = cols.positions.len() as u32;
                    for (a, y) in [(a0, 0.25), (a1, 0.25), (a1, FLOOR_H), (a0, FLOOR_H)] {
                        cols.positions.push(Vec3::new(
                            px as f32 + a.cos() * r,
                            y,
                            pz as f32 + a.sin() * r,
                        ));
                    }
                    cols
                        .uv
                        .extend_from_slice(&[[0.0, 0.0], [0.2, 0.0], [0.2, 1.0], [0.0, 1.0]]);
                    cols.indices.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
                }
                let _ = base;
            }
        }
        parts.push(cols);
    }
    parts.retain(|p| !p.indices.is_empty());
    parts
}

fn len_acc(v: &[Vec3]) -> usize {
    v.len()
}

/// Site block (x, z, radius, commercial).
pub type Site = (f64, f64, f64, bool);

/// Scatter logic-restricted city blocks around each site: jittered grid,
/// rejected when too close to a road corridor or overlapping an accepted
/// block. Returns the merged building mesh parts (world space).
pub fn blocks_for_sites(curves: &[RoadCurve], sites: &[Site], seed: u64) -> Vec<MeshPart> {
    let mut rng = Lcg(if seed == 0 { 0x9E3779B9 } else { seed });
    // road samples for the distance gate (points + paved half-width)
    let roads: Vec<(f64, f64, f64)> = curves
        .iter()
        .flat_map(|c| c.points.iter().map(move |p| (p.x, p.z, c.paved_width() / 2.0)))
        .collect();
    let mut accepted: Vec<(f64, f64, f64)> = Vec::new(); // (x, z, cull radius)
    let mut out: Vec<MeshPart> = Vec::new();
    let mut seed_counter = seed;
    for &(sx, sz, radius, commercial) in sites {
        let step = if commercial { 30.0 } else { 24.0 };
        let n = (radius / step).ceil() as i64;
        for gx in -n..=n {
            for gz in -n..=n {
                let bx = sx + gx as f64 * step + rng.range(-6.0, 6.0);
                let bz = sz + gz as f64 * step + rng.range(-6.0, 6.0);
                if (bx - sx).hypot(bz - sz) > radius {
                    continue;
                }
                let (w, d, floors) = if commercial {
                    (rng.range(13.0, 24.0), rng.range(10.0, 17.0), rng.range(1.9, 4.4))
                } else {
                    (rng.range(8.0, 14.0), rng.range(7.0, 12.0), rng.range(1.0, 3.6))
                };
                let setback = 5.5;
                // road corridor gate
                let mut blocked = false;
                for (rx, rz, rhalf) in &roads {
                    if (bx - rx).hypot(bz - rz) < rhalf + setback + w.max(d) * 0.5 {
                        blocked = true;
                        break;
                    }
                }
                if blocked {
                    continue;
                }
                // overlap gate (cull circles)
                let cull = (w.max(d) * 0.78) as f64;
                if accepted
                    .iter()
                    .any(|(ax, az, ar)| (bx - ax).hypot(bz - az) < cull + ar)
                {
                    continue;
                }
                accepted.push((bx, bz, cull));
                seed_counter += 0x2545F491;
                let yaw = rng.range(0.0, std::f64::consts::TAU);
                for part in building_parts(bx, bz, w, d, floors as usize, yaw, seed_counter) {
                    out.push(part);
                }
                if out.len() > 90_000 {
                    // hard triangle budget guard (positions/3 ≈ tris)
                    return out;
                }
            }
        }
    }
    out
}
