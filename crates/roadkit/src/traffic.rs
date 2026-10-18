//! Programmatic GLB generators — the traffic-signal / road-sign / BRT asset pack.
//!
//! Same hand-rolled, dependency-free GLB writer style as the rest of roadkit:
//! every mesh is built here in code (low-poly, vertex-coloured) and packed into
//! a binary glTF 2.0 container identical in layout to the five existing assets
//! in `assets/glb/` (single node + mesh + primitive, POSITION/NORMAL/COLOR_0
//! f32 VEC3, non-indexed triangles, `vertexcolor` material).
//!
//! Orientation convention (matches the existing props under the join code):
//! the joiner yaw-aligns props to the curve tangent via `Mat4::rot_y(yaw)`,
//! which at yaw = 0 is the IDENTITY — model +X = east, +Y = up, +Z = south.
//! Along-travel therefore runs along model X on the axis-aligned roads the
//! scripts stamp on, and across-travel along model Z. All DIRECTIONAL assets
//! here (signal heads, sign plates) face **-X** — i.e. back against the
//! travel direction of an eastbound road, exactly where oncoming drivers are.
//! Origin sits at ground level, y-up, meters.

use serde_json::json;
use std::path::Path;

pub const GENERATOR: &str = "Terrazone roadkit asset builder";

/* ------------------------------------------------------------------ */
/* palette                                                             */
/* ------------------------------------------------------------------ */

const fn rgb(r: f32, g: f32, b: f32) -> [f32; 4] {
    [r, g, b, 1.0]
}
const RED: [f32; 4] = rgb(0.82, 0.10, 0.10);
const AMBER: [f32; 4] = rgb(0.95, 0.62, 0.05);
const GREEN: [f32; 4] = rgb(0.10, 0.62, 0.20);
const POLE_DARK: [f32; 4] = rgb(0.10, 0.10, 0.12);
const STEEL: [f32; 4] = rgb(0.34, 0.36, 0.40);
const POST_GREY: [f32; 4] = rgb(0.55, 0.56, 0.58);
const WHITE: [f32; 4] = rgb(0.92, 0.92, 0.90);
const BLACK: [f32; 4] = rgb(0.05, 0.05, 0.06);
const SIGN_YELLOW: [f32; 4] = rgb(0.95, 0.78, 0.10);
const SIGN_BLUE: [f32; 4] = rgb(0.10, 0.25, 0.60);
const CONCRETE: [f32; 4] = rgb(0.72, 0.71, 0.68);
const TACTILE_RED: [f32; 4] = rgb(0.70, 0.12, 0.10);
const ROOF_GREY: [f32; 4] = rgb(0.30, 0.31, 0.34);

/* ------------------------------------------------------------------ */
/* mesh builder                                                        */
/* ------------------------------------------------------------------ */

/// Accumulates non-indexed triangles with flat normals + vertex colors.
#[derive(Default)]
pub struct MeshBuilder {
    pos: Vec<[f32; 3]>,
    nrm: Vec<[f32; 3]>,
    col: Vec<[f32; 4]>,
}

impl MeshBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], color: [f32; 4]) {
        // flat normal (degenerate falls back to +Y)
        let (ux, uy, uz) = (b[0] - a[0], b[1] - a[1], b[2] - a[2]);
        let (vx, vy, vz) = (c[0] - a[0], c[1] - a[1], c[2] - a[2]);
        let mut n = [
            uy * vz - uz * vy,
            uz * vx - ux * vz,
            ux * vy - uy * vx,
        ];
        let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
        n = if len > 1e-9 {
            [n[0] / len, n[1] / len, n[2] / len]
        } else {
            [0.0, 1.0, 0.0]
        };
        for p in [a, b, c] {
            self.pos.push(p);
            self.nrm.push(n);
            self.col.push(color);
        }
    }

    /// One triangle.
    pub fn tri(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], color: [f32; 4]) {
        self.push(a, b, c, color);
    }

    /// Planar quad (two triangles).
    pub fn quad(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], color: [f32; 4]) {
        self.push(a, b, c, color);
        self.push(a, c, d, color);
    }

    /// Axis-aligned box from opposite corners.
    pub fn cuboid(&mut self, min: [f32; 3], max: [f32; 3], color: [f32; 4]) {
        let [x0, y0, z0] = min;
        let [x1, y1, z1] = max;
        self.quad([x1, y0, z0], [x1, y0, z1], [x1, y1, z1], [x1, y1, z0], color); // +x
        self.quad([x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0], color); // -x
        self.quad([x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0], color); // +y
        self.quad([x0, y0, z0], [x0, y0, z1], [x1, y0, z1], [x1, y0, z0], color); // -y
        self.quad([x0, y0, z1], [x0, y1, z1], [x1, y1, z1], [x1, y0, z1], color); // +z
        self.quad([x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0], color); // -z
    }

    /// Vertical post: square column centered at (cx, cz), half-width `r`.
    fn post(&mut self, cx: f32, cz: f32, r: f32, y0: f32, y1: f32, color: [f32; 4]) {
        self.cuboid([cx - r, y0, cz - r], [cx + r, y1, cz + r], color);
    }

    /// Flat polygon in the YZ plane at depth `x` (a sign plate), fanned from
    /// vertex 0; `double` adds the reverse face.
    fn plate(&mut self, x: f32, pts: &[[f32; 2]], color: [f32; 4], double: bool) {
        let p0 = [x, pts[0][0], pts[0][1]];
        for i in 1..pts.len() - 1 {
            let a = [x, pts[i][0], pts[i][1]];
            let b = [x, pts[i + 1][0], pts[i + 1][1]];
            self.tri(p0, a, b, color);
        }
        if double {
            for i in 1..pts.len() - 1 {
                let a = [x, pts[i][0], pts[i][1]];
                let b = [x, pts[i + 1][0], pts[i + 1][1]];
                self.tri(p0, b, a, color);
            }
        }
    }

    /// Regular n-gon centered (cy, cz) in the YZ plane.
    fn ngon(&mut self, x: f32, cy: f32, cz: f32, r: f32, sides: usize, rot: f32, color: [f32; 4], double: bool) {
        let pts: Vec<[f32; 2]> = (0..sides)
            .map(|k| {
                let a = rot + (k as f32) * std::f32::consts::TAU / sides as f32;
                [cy + r * a.cos(), cz + r * a.sin()]
            })
            .collect();
        self.plate(x, &pts, color, double);
    }

    /// Rectangle quad facing -X (in the YZ plane): signal lens / paint bar.
    fn face_rect(&mut self, x: f32, y0: f32, y1: f32, z0: f32, z1: f32, color: [f32; 4]) {
        self.quad([x, y0, z0], [x, y0, z1], [x, y1, z1], [x, y1, z0], color);
    }

    /// Sloped quad across X (ramp face): from (x0,y0) to (x1,y1) spanning z.
    fn ramp(&mut self, x0: f32, y0: f32, x1: f32, y1: f32, z0: f32, z1: f32, color: [f32; 4]) {
        self.quad([x0, y0, z0], [x0, y0, z1], [x1, y1, z1], [x1, y1, z0], color);
    }

    /// 7-segment digit painted on a -X-facing plate (bar quads, "50"/"80").
    fn digit(&mut self, x: f32, zc: f32, yc: f32, h: f32, w: f32, t: f32, d: u8, color: [f32; 4]) {
        // segment centers: a top, g middle, d bottom; f/e left, b/c right
        let (hw, hh) = (w * 0.5, h * 0.5);
        let mut bars: Vec<[f32; 4]> = Vec::new(); // (y0, y1, z0, z1)
        let has = |s: u8, mask: u8| s & mask != 0;
        // a=1 b=2 c=4 d=8 e=16 f=32 g=64
        if has(d, 1) {
            bars.push([yc + hh - t, yc + hh, zc - hw + t, zc + hw - t]);
        }
        if has(d, 64) {
            bars.push([yc - t * 0.5, yc + t * 0.5, zc - hw + t, zc + hw - t]);
        }
        if has(d, 8) {
            bars.push([yc - hh, yc - hh + t, zc - hw + t, zc + hw - t]);
        }
        if has(d, 2) {
            bars.push([yc + t * 0.5, yc + hh - t * 0.5, zc + hw - t, zc + hw]);
        }
        if has(d, 4) {
            bars.push([yc - hh + t * 0.5, yc - t * 0.5, zc + hw - t, zc + hw]);
        }
        if has(d, 32) {
            bars.push([yc + t * 0.5, yc + hh - t * 0.5, zc - hw, zc - hw + t]);
        }
        if has(d, 16) {
            bars.push([yc - hh + t * 0.5, yc - t * 0.5, zc - hw, zc - hw + t]);
        }
        for b in bars {
            self.face_rect(x, b[0], b[1], b[2], b[3], color);
        }
    }

    pub fn tri_count(&self) -> usize {
        self.pos.len() / 3
    }
}

/* ------------------------------------------------------------------ */
/* GLB writer                                                          */
/* ------------------------------------------------------------------ */

/// Pack the accumulated triangles into a binary glTF 2.0 (GLB) container,
/// byte-layout identical to the five existing roadkit assets.
pub fn to_glb(mb: &MeshBuilder) -> Vec<u8> {
    let n = mb.pos.len() as u32;
    let view_len = n * 12;
    let mut bin: Vec<u8> = Vec::with_capacity(view_len as usize * 3);
    for v in &mb.pos {
        bin.extend_from_slice(&v[0].to_le_bytes());
        bin.extend_from_slice(&v[1].to_le_bytes());
        bin.extend_from_slice(&v[2].to_le_bytes());
    }
    for v in &mb.nrm {
        bin.extend_from_slice(&v[0].to_le_bytes());
        bin.extend_from_slice(&v[1].to_le_bytes());
        bin.extend_from_slice(&v[2].to_le_bytes());
    }
    for v in &mb.col {
        bin.extend_from_slice(&v[0].to_le_bytes());
        bin.extend_from_slice(&v[1].to_le_bytes());
        bin.extend_from_slice(&v[2].to_le_bytes());
        // alpha is implicit 1.0 (COLOR_0 VEC3, like the existing assets)
    }

    let mut min = [f32::MAX; 3];
    let mut max = [f32::MIN; 3];
    for v in &mb.pos {
        for k in 0..3 {
            min[k] = min[k].min(v[k]);
            max[k] = max[k].max(v[k]);
        }
    }

    let json = json!({
        "asset": { "version": "2.0", "generator": GENERATOR },
        "scene": 0,
        "scenes": [ { "nodes": [0], "name": "Scene" } ],
        "nodes": [ { "mesh": 0, "name": "asset" } ],
        "meshes": [ {
            "primitives": [ {
                "attributes": { "POSITION": 0, "NORMAL": 1, "COLOR_0": 2 },
                "mode": 4,
                "material": 0
            } ]
        } ],
        "accessors": [
            { "bufferView": 0, "componentType": 5126, "count": n, "type": "VEC3",
              "min": min, "max": max },
            { "bufferView": 1, "componentType": 5126, "count": n, "type": "VEC3" },
            { "bufferView": 2, "componentType": 5126, "count": n, "type": "VEC3" }
        ],
        "bufferViews": [
            { "buffer": 0, "byteOffset": 0, "byteLength": view_len, "target": 34962 },
            { "buffer": 0, "byteOffset": view_len, "byteLength": view_len, "target": 34962 },
            { "buffer": 0, "byteOffset": view_len * 2, "byteLength": view_len, "target": 34962 }
        ],
        "materials": [ {
            "name": "vertexcolor",
            "pbrMetallicRoughness": {
                "baseColorFactor": [1.0, 1.0, 1.0, 1.0],
                "metallicFactor": 0.1,
                "roughnessFactor": 0.8
            }
        } ],
        "buffers": [ { "byteLength": view_len * 3 } ]
    });
    let mut json_bytes = serde_json::to_vec(&json).expect("json serialize");
    while json_bytes.len() % 4 != 0 {
        json_bytes.push(b' ');
    }
    while bin.len() % 4 != 0 {
        bin.push(0);
    }

    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(&0x4654_6C67u32.to_le_bytes()); // "glTF"
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F_534Au32.to_le_bytes()); // "JSON"
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E_4942u32.to_le_bytes()); // "BIN\0"
    out.extend_from_slice(&bin);
    out
}

/* ------------------------------------------------------------------ */
/* traffic lights                                                      */
/* ------------------------------------------------------------------ */

/// Tanzanian junction signal: dark pole, backboard, vertical red/amber/green
/// head facing -X (toward oncoming traffic), plus the smaller second head
/// many TZ junctions carry at mid height.
pub fn traffic_light_tz() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    // base + pole
    m.cuboid([-0.16, 0.0, -0.16], [0.16, 0.25, 0.16], POLE_DARK);
    m.post(0.0, 0.0, 0.07, 0.0, 4.2, POLE_DARK);
    // main head: backboard + housing, facing -X
    m.cuboid([-0.10, 2.55, -0.45], [0.0, 4.05, 0.45], POLE_DARK);
    m.cuboid([-0.42, 2.60, -0.36], [-0.10, 4.00, 0.36], POLE_DARK);
    // lenses (front at x = -0.425): red / amber / green top-down
    m.face_rect(-0.425, 3.60, 3.92, -0.17, 0.17, RED);
    m.face_rect(-0.425, 3.16, 3.48, -0.17, 0.17, AMBER);
    m.face_rect(-0.425, 2.72, 3.04, -0.17, 0.17, GREEN);
    // second (smaller) head, same facing, mid-pole
    m.cuboid([-0.32, 1.55, -0.24], [-0.10, 2.35, 0.24], POLE_DARK);
    m.face_rect(-0.325, 2.06, 2.26, -0.11, 0.11, RED);
    m.face_rect(-0.325, 1.83, 2.03, -0.11, 0.11, AMBER);
    m.face_rect(-0.325, 1.60, 1.80, -0.11, 0.11, GREEN);
    m
}

/// US-style signal: tall pole with a horizontal mast arm reaching over the
/// road (+X), wide visored hoods, head hanging over the lane.
pub fn traffic_light_usa() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.cuboid([-0.18, 0.0, -0.18], [0.18, 0.3, 0.18], STEEL);
    m.post(0.0, 0.0, 0.11, 0.0, 5.6, STEEL);
    // mast arm over the road
    m.cuboid([0.0, 5.28, -0.09], [4.6, 5.52, 0.09], STEEL);
    // head on the arm: backplate + housing facing -X
    m.cuboid([3.86, 3.60, -0.55], [3.92, 5.10, 0.55], POLE_DARK);
    m.cuboid([3.62, 3.66, -0.42], [3.86, 5.04, 0.42], POLE_DARK);
    // wide lenses + visor hoods (angled caps over each aspect)
    for (i, col) in [RED, AMBER, GREEN].into_iter().enumerate() {
        let y0 = 4.62 - i as f32 * 0.37;
        m.face_rect(3.615, y0, y0 + 0.36, -0.19, 0.19, col);
        m.ramp(3.615, y0 + 0.37, 3.50, y0 + 0.53, -0.20, 0.20, POLE_DARK);
    }
    m
}

/// European signal: slender pole, compact 3-aspect head with louvres, and the
/// separate red/green pedestrian man low on the pole.
pub fn traffic_light_eu() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.post(0.0, 0.0, 0.05, 0.0, 3.6, STEEL);
    // vehicle head
    m.cuboid([-0.36, 2.50, -0.29], [-0.06, 3.56, 0.29], POLE_DARK);
    for (i, col) in [RED, AMBER, GREEN].into_iter().enumerate() {
        let y0 = 3.24 - i as f32 * 0.26;
        m.face_rect(-0.365, y0, y0 + 0.24, -0.14, 0.14, col);
        m.ramp(-0.365, y0 + 0.245, -0.44, y0 + 0.36, -0.15, 0.15, POLE_DARK); // louvre
    }
    // pedestrian signal low on the pole
    m.cuboid([-0.17, 1.00, -0.15], [-0.06, 1.55, 0.15], POLE_DARK);
    m.face_rect(-0.175, 1.32, 1.48, -0.08, 0.08, RED); // red man
    m.face_rect(-0.175, 1.06, 1.22, -0.08, 0.08, GREEN); // green man
    m
}

/* ------------------------------------------------------------------ */
/* signs                                                               */
/* ------------------------------------------------------------------ */

fn octagon(cy: f32, cz: f32, r: f32) -> Vec<[f32; 2]> {
    (0..8)
        .map(|k| {
            let a = std::f32::consts::FRAC_PI_8 + (k as f32) * std::f32::consts::FRAC_PI_4;
            [cy + r * a.cos(), cz + r * a.sin()]
        })
        .collect()
}

fn triangle(cy: f32, cz: f32, r: f32, point_up: bool) -> Vec<[f32; 2]> {
    let base = if point_up {
        std::f32::consts::FRAC_PI_2
    } else {
        -std::f32::consts::FRAC_PI_2
    };
    (0..3)
        .map(|k| {
            let a = base + (k as f32) * std::f32::consts::TAU / 3.0;
            [cy + r * a.cos(), cz + r * a.sin()]
        })
        .collect()
}

/// Red octagonal STOP sign with white rim + letter-bar impression.
pub fn sign_stop() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.plate(0.0, &octagon(2.35, 0.0, 0.45), WHITE, true);
    m.plate(-0.014, &octagon(2.35, 0.0, 0.405), RED, true);
    // four white bars — letter impression
    for k in 0..4i32 {
        let z = -0.195 + k as f32 * 0.13;
        m.face_rect(-0.022, 2.32, 2.38, z - 0.045, z + 0.045, WHITE);
    }
    m.post(0.0, 0.0, 0.04, 0.0, 2.35, POST_GREY);
    m
}

/// Inverted-triangle GIVE WAY / YIELD sign, white on red.
pub fn sign_yield() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.plate(0.0, &triangle(2.4, 0.0, 0.50, false), RED, true);
    m.plate(-0.014, &triangle(2.4, 0.0, 0.42, false), WHITE, true);
    m.post(0.0, 0.0, 0.04, 0.0, 1.95, POST_GREY);
    m
}

fn speed_sign(digits: [u8; 2]) -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.ngon(0.0, 2.4, 0.0, 0.45, 12, 0.0, RED, true);
    m.ngon(-0.014, 2.4, 0.0, 0.375, 12, 0.0, WHITE, true);
    for (i, d) in digits.into_iter().enumerate() {
        m.digit(-0.024, -0.105 + i as f32 * 0.21, 2.4, 0.24, 0.13, 0.035, d, BLACK);
    }
    m.post(0.0, 0.0, 0.04, 0.0, 1.95, POST_GREY);
    m
}

/// 7-seg bit masks: a b c d e f g
pub const DIGIT_5: u8 = 0b010_0101; // a f g c d
pub const DIGIT_8: u8 = 0b111_1111;
pub const DIGIT_0: u8 = 0b011_1111; // a b c d e f

/// Circular speed-limit sign, red ring, black "50".
pub fn sign_speed_50() -> MeshBuilder {
    speed_sign([DIGIT_5, DIGIT_0])
}

/// Circular speed-limit sign, red ring, black "80".
pub fn sign_speed_80() -> MeshBuilder {
    speed_sign([DIGIT_8, DIGIT_0])
}

/// No-entry: red disc, white rim, white horizontal bar.
pub fn sign_no_entry() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.ngon(0.0, 2.4, 0.0, 0.47, 12, 0.0, WHITE, true);
    m.ngon(-0.014, 2.4, 0.0, 0.41, 12, 0.0, RED, true);
    m.face_rect(-0.022, 2.33, 2.47, -0.28, 0.28, WHITE);
    m.post(0.0, 0.0, 0.04, 0.0, 1.95, POST_GREY);
    m
}

/// Triangular warning sign, yellow with black border.
pub fn sign_warning() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.plate(0.0, &triangle(2.4, 0.0, 0.50, true), SIGN_YELLOW, true);
    m.plate(-0.014, &triangle(2.4, 0.0, 0.43, true), BLACK, true);
    m.plate(-0.024, &triangle(2.4, 0.0, 0.36, true), SIGN_YELLOW, true);
    m.post(0.0, 0.0, 0.04, 0.0, 1.95, POST_GREY);
    m
}

/// Blank blue street-name blade (UI/scripts caption it) on a thin post.
pub fn sign_road_name() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    m.cuboid([-0.03, 2.40, -0.85], [0.0, 2.80, 0.85], WHITE);
    m.cuboid([-0.055, 2.43, -0.81], [-0.03, 2.77, 0.81], SIGN_BLUE);
    // two pale dashes as caption placeholders
    m.face_rect(-0.058, 2.58, 2.62, -0.55, -0.15, WHITE);
    m.face_rect(-0.058, 2.58, 2.62, 0.15, 0.55, WHITE);
    m.post(0.0, 0.0, 0.035, 0.0, 2.6, POST_GREY);
    m
}

/// Pedestrian crossing sign (blue plate + walking-man bars) with a short
/// zebra-striped bollard beside it.
pub fn sign_pedestrian() -> MeshBuilder {
    let mut m = MeshBuilder::new();
    // sign plate
    m.cuboid([-0.02, 1.70, -0.30], [0.0, 2.30, 0.30], WHITE);
    m.cuboid([-0.045, 1.73, -0.27], [-0.02, 2.27, 0.27], SIGN_BLUE);
    // walking-man impression: head / body / legs
    m.face_rect(-0.048, 2.08, 2.16, -0.02, 0.06, WHITE);
    m.face_rect(-0.048, 1.86, 2.06, -0.02, 0.04, WHITE);
    m.face_rect(-0.048, 1.78, 1.88, -0.12, 0.06, WHITE);
    m.post(0.0, 0.0, 0.035, 0.0, 2.0, POST_GREY);
    // zebra bollard just beside the sign post
    m.cuboid([-0.05, 0.0, 0.70], [0.05, 0.90, 0.80], WHITE);
    m.face_rect(-0.055, 0.28, 0.42, 0.70, 0.80, BLACK);
    m.face_rect(-0.055, 0.56, 0.70, 0.70, 0.80, BLACK);
    m
}

/* ------------------------------------------------------------------ */
/* BRT (Mwendokasi) median stations                                    */
/* ------------------------------------------------------------------ */

fn brt_platform(platform_len: f32, roof_len: f32, canopy_h: f32, columns: &[f32], col_z: f32) -> MeshBuilder {
    let mut m = MeshBuilder::new();
    let hw = platform_len * 0.5;
    // concrete platform slab sitting in the median
    m.cuboid([-hw, 0.0, -1.5], [hw, 0.5, 1.5], CONCRETE);
    // tactile red edge strips both sides
    m.cuboid([-hw, 0.5, -1.5], [hw, 0.52, -1.28], TACTILE_RED);
    m.cuboid([-hw, 0.5, 1.28], [hw, 0.52, 1.5], TACTILE_RED);
    // ramp ends down to the roadway
    m.ramp(hw, 0.5, hw + 1.4, 0.0, -0.9, 0.9, CONCRETE);
    m.ramp(-hw, 0.5, -hw - 1.4, 0.0, -0.9, 0.9, CONCRETE);
    // steel canopy columns
    for &x in columns {
        m.post(x, -col_z, 0.06, 0.5, canopy_h, STEEL);
        m.post(x, col_z, 0.06, 0.5, canopy_h, STEEL);
    }
    // canopy roof + red brand fascia
    let rw = roof_len * 0.5;
    m.cuboid([-rw, canopy_h, -1.95], [rw, canopy_h + 0.16, 1.95], ROOF_GREY);
    m.cuboid([-rw, canopy_h - 0.12, -2.0], [rw, canopy_h + 0.02, -1.9], TACTILE_RED);
    m.cuboid([-rw, canopy_h - 0.12, 1.9], [rw, canopy_h + 0.02, 2.0], TACTILE_RED);
    // fare gates + bench on the platform
    m.cuboid([-platform_len * 0.2, 0.5, -0.3], [-platform_len * 0.2 + 0.5, 1.25, 0.3], STEEL);
    m.cuboid([platform_len * 0.2 - 0.5, 0.5, -0.3], [platform_len * 0.2, 1.25, 0.3], STEEL);
    m.cuboid([-1.8, 0.5, -0.55], [1.8, 0.72, -0.35], ROOF_GREY);
    m
}

/// Dar es Salaam BRT (Mwendokasi) median station: ~30 m platform, canopy on
/// steel columns, tactile-red edge strips, ramp ends. Fits a 3.2 m median.
pub fn brt_station() -> MeshBuilder {
    brt_platform(30.0, 26.0, 3.4, &[-10.0, 0.0, 10.0], 1.25)
}

/// Short 12 m BRT shelter variant.
pub fn brt_shelter_small() -> MeshBuilder {
    brt_platform(12.0, 9.0, 3.0, &[-3.5, 3.5], 1.15)
}

/* ------------------------------------------------------------------ */
/* registry                                                            */
/* ------------------------------------------------------------------ */

/// One generated asset: manifest id, file name, role and GLB payload.
pub struct TrafficAsset {
    pub id: &'static str,
    pub file: &'static str,
    pub role: &'static str,
    pub bytes: Vec<u8>,
}

/// Build the whole traffic + BRT pack in memory.
pub fn build_all() -> Vec<TrafficAsset> {
    let makers: &[(&'static str, &'static str, &'static str, fn() -> MeshBuilder)] = &[
        ("trafficLightTz", "traffic_light_tz.glb", "traffic signal", traffic_light_tz),
        ("trafficLightUsa", "traffic_light_usa.glb", "traffic signal", traffic_light_usa),
        ("trafficLightEu", "traffic_light_eu.glb", "traffic signal", traffic_light_eu),
        ("signStop", "sign_stop.glb", "traffic sign", sign_stop),
        ("signYield", "sign_yield.glb", "traffic sign", sign_yield),
        ("signSpeed50", "sign_speed_50.glb", "traffic sign", sign_speed_50),
        ("signSpeed80", "sign_speed_80.glb", "traffic sign", sign_speed_80),
        ("signNoEntry", "sign_no_entry.glb", "traffic sign", sign_no_entry),
        ("signWarning", "sign_warning.glb", "traffic sign", sign_warning),
        ("signRoadName", "sign_road_name.glb", "traffic sign", sign_road_name),
        ("signPedestrian", "sign_pedestrian.glb", "traffic sign", sign_pedestrian),
        ("brtStation", "brt_station.glb", "brt station", brt_station),
        ("brtShelterSmall", "brt_shelter_small.glb", "brt station", brt_shelter_small),
    ];
    makers
        .iter()
        .map(|(id, file, role, make)| TrafficAsset {
            id,
            file,
            role,
            bytes: to_glb(&make()),
        })
        .collect()
}

/// Write the pack into an assets dir (the `glb/` folder), replacing files.
pub fn write_to(glb_dir: &Path, assets: &[TrafficAsset]) -> std::io::Result<()> {
    std::fs::create_dir_all(glb_dir)?;
    for a in assets {
        std::fs::write(glb_dir.join(a.file), &a.bytes)?;
    }
    Ok(())
}

/* ------------------------------------------------------------------ */
/* tests                                                               */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;
    use crate::glb::parse_glb;

    const TRI_BUDGET: usize = 600;

    #[test]
    fn pack_builds_and_parses() {
        let assets = build_all();
        assert_eq!(assets.len(), 13);
        for a in &assets {
            let mesh = parse_glb(&a.bytes, a.id).unwrap_or_else(|e| panic!("{}: {e}", a.id));
            let tris = mesh.triangle_count();
            assert!(tris > 0, "{} has no triangles", a.id);
            assert!(tris <= TRI_BUDGET, "{} too heavy: {tris} tris", a.id);
            // origin at ground: nothing below y = 0 (1 cm tolerance)
            assert!(mesh.min.y > -0.011, "{} dips below ground: {}", a.id, mesh.min.y);
        }
    }

    #[test]
    fn tri_counts_are_low_poly() {
        for a in build_all() {
            let tris = crate::glb::parse_glb(&a.bytes, a.id).unwrap().triangle_count();
            println!("{:>18}  {:>3} tris  {} bytes", a.id, tris, a.bytes.len());
        }
    }

    #[test]
    fn stations_are_long_enough_for_median() {
        let big = crate::glb::parse_glb(&to_glb(&brt_station()), "brtStation").unwrap();
        assert!(big.max.x - big.min.x > 29.0, "station platform too short");
        assert!(big.max.z - big.min.z < 4.6, "station too wide for a 3.2 m median");
        let small = crate::glb::parse_glb(&to_glb(&brt_shelter_small()), "small").unwrap();
        assert!(small.max.x - small.min.x > 11.0);
    }
}
