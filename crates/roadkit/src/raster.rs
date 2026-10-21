//! Tiny CPU software rasterizer (z-buffered, flat-shaded, vertex colors).
//! Renders the 3D road scene + GLB props to RGBA pixels that the egui UI
//! uploads as a texture. No GPU dependency — works everywhere eframe runs.
//!
//! R10 adds Blender-style viewport shading: Solid (default), Normals
//! (world-normal RGB) and Wireframe (hidden-line: dark depth fill +
//! bright edges) — see `ShadeMode`.
//!
//! R15 remakes the environment after the Egregoria engine (uriopass/
//! egregoria, MIT): when `Frame::env_mode` is on, fragments are shaded in
//! LINEAR space (`albedo²` gamma-2.0 approximation) by the analytic sun
//! (engine::sun — `sun_col = 4·(sun.y)^0.25·(1, 0.95+0.05·sun.y, …)`),
//! receive single-cascade sun shadows, and get Egregoria's aerial
//! perspective ADDITIVELY (`color += fog`) from a tiny per-camera
//! `engine::atmosphere::FogLut` — the same single-scattering atmosphere
//! that fills the sky LUT, so ground, water and sky all melt into one
//! haze. The final `compose_env` applies Egregoria's autoexposure (×10
//! at night), the `1.83c/(1.24c+1.68)` tonemap and hash dithering, then
//! gamma-2.0 encodes. Water is a separate z-tested pass with Gerstner
//! wave normals (engine::water) and an atmosphere reflection. This is
//! the wgpu forward pipeline (depth prepass → shadow → forward → sky)
//! flattened onto the CPU raster.

use std::sync::Arc;

use engine::atmosphere::{FogLut, SkyLut};
use engine::water::{gerstner_wave_normal_unit, shore_factor, WATER_BASE};
use gdmath::mat4::Mat4;
use gdmath::vec3::Vec3;

/// One-cascade sun shadow map (Egregoria uses 4; the CPU budget keeps the
/// nearest one — the cascade that carries most of the visible look).
/// Depth stores light-space view-z (more negative = farther from the sun).
pub struct ShadowData {
    pub size: usize,
    pub depth: Vec<f32>,
    /// light-space view matrix (world → light view)
    pub view: Mat4,
    /// ortho half-extent in meters (map covers [-ext, ext]² around target)
    pub ext: f32,
    /// ortho near/far planes in light view space (z <= -near is kept)
    pub near: f32,
    pub far: f32,
}

impl ShadowData {
    /// Sample the shadow factor at a world position (0 = shadowed, 1 = lit).
    /// 3×3 PCF when `pcf`, else a single biased tap. Outside the map = lit.
    pub fn test(&self, wpos: Vec3, n_dot_l: f32, pcf: bool) -> f32 {
        let lv = self.view.transform_point(wpos).0;
        if lv.z >= -self.near || lv.z <= -self.far {
            return 1.0;
        }
        let u = lv.x / self.ext * 0.5 + 0.5;
        let v = lv.y / self.ext * 0.5 + 0.5;
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return 1.0;
        }
        // slope-scaled bias (light-space z units are meters) — generous
        // because the shadow map is 1.5-2 m/texel at corridor scale
        let bias = 0.6 + 1.6 * (1.0 - n_dot_l.clamp(0.0, 1.0));
        let frag_z = -lv.z;
        let x = (u * self.size as f32) as isize;
        let y = (v * self.size as f32) as isize;
        let tap = |tx: isize, ty: isize| -> f32 {
            if tx < 0 || ty < 0 || tx >= self.size as isize || ty >= self.size as isize {
                return 1.0;
            }
            let stored = -self.depth[(ty as usize) * self.size + tx as usize];
            if stored == f32::NEG_INFINITY {
                return 1.0; // nothing rendered there = lit
            }
            if frag_z > stored + bias {
                0.0
            } else {
                1.0
            }
        };
        if !pcf {
            return tap(x, y);
        }
        let mut sum = 0.0f32;
        for dy in -1..=1 {
            for dx in -1..=1 {
                sum += tap(x + dx, y + dy);
            }
        }
        sum / 9.0
    }
}

/// Viewport shading modes (Blender solid / normal / wireframe).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum ShadeMode {
    #[default]
    Solid,
    /// RGB = world normal × 0.5 + 0.5 (no texture)
    Normals,
    /// hidden-line wireframe (depth fill + bright edges)
    Wireframe,
    /// solid (textured) fill + bright edge overlay
    ShadedWire,
    /// grayscale view-depth (white near → black far)
    Depth,
    /// 2× SSAA supersampled Solid (handled by the render entry points)
    Quality,
}

pub struct Frame {
    pub w: usize,
    pub h: usize,
    /// rgba8
    pub color: Vec<u8>,
    /// nearest view-space z per pixel (more negative = farther)
    pub depth: Vec<f32>,
    /// distance fog: blend toward color from start..end view depth (meters)
    pub fog_start: f32,
    pub fog_end: f32,
    pub fog_color: [f32; 3],
    pub fog_on: bool,
    /// active shading mode (Blender-style viewport modes)
    pub shade: ShadeMode,
    /// (near, far) view distance in meters for the Depth mode gradient
    pub depth_range: (f32, f32),
    /// parallel rendering: this frame covers the horizontal band of the FULL
    /// image starting at `y_base`; projection maps into FULL-image pixels then
    /// shifts by `-y_base` so local pixel (0,0) is full-image (0, y_base).
    pub y_base: usize,
    pub full_h: usize,
    /// R15 Egregoria environment mode: shade linear + sun + atmosphere fog;
    /// color goes to `hdr` (w*h*3) and is composed by `compose_env`.
    pub env_mode: bool,
    /// linear HDR radiance (w*h*3), only written when env_mode
    pub hdr: Vec<f32>,
    /// sky LUT (rebuilt when the sun moves) for water reflections + sky fill
    pub sky: Option<Arc<SkyLut>>,
    /// aerial-perspective LUT (rebuilt per camera/sun change)
    pub fog_lut: Option<Arc<FogLut>>,
    /// linear sun color × intensity (engine::sun::sun_color rgb)
    pub sun_col: [f32; 3],
    /// camera eye (world) for ray-based fog sampling
    pub eye: Vec3,
    /// wave animation clock (seconds)
    pub env_time: f32,
    /// draw elevation contour lines on ground fragments (10 m / 50 m)
    pub contours: bool,
    /// current draw is GROUND (contours + no self-shadow acne bias)
    pub ground_pass: bool,
    /// sun shadow map (None = disabled)
    pub shadow: Option<Arc<ShadowData>>,
    /// current mesh is the WATER plane (Gerstner + atmosphere reflection)
    pub water: bool,
    /// depth-only rendering (shadow map pass) — no color writes
    pub depth_only: bool,
    /// normalized light (sun) direction for env shading
    pub light: Vec3,
    /// EDITOR floor: output the raw albedo unlit (exactly the backdrop
    /// color after the compose tonemap — no sun, no fog, no shadows)
    pub flat_floor: bool,
}

impl Frame {
    pub fn new(w: usize, h: usize, bg: [u8; 4]) -> Self {
        Self::band(w, h, bg, 0, h)
    }

    /// A horizontal band of a taller full image (thread-per-band rendering).
    pub fn band(w: usize, h: usize, bg: [u8; 4], y_base: usize, full_h: usize) -> Self {
        let mut f = Self {
            w,
            h,
            color: vec![0; w * h * 4],
            depth: vec![f32::NEG_INFINITY; w * h],
            fog_start: 1.0e9,
            fog_end: 2.0e9,
            fog_color: [
                bg[0] as f32 / 255.0,
                bg[1] as f32 / 255.0,
                bg[2] as f32 / 255.0,
            ],
            fog_on: false,
            shade: ShadeMode::Solid,
            depth_range: (5.0, 600.0),
            y_base,
            full_h,
            env_mode: false,
            hdr: vec![0.0; w * h * 3],
            sky: None,
            fog_lut: None,
            sun_col: [1.0, 1.0, 1.0],
            eye: Vec3::ZERO,
            env_time: 0.0,
            contours: false,
            ground_pass: false,
            shadow: None,
            water: false,
            depth_only: false,
            light: Vec3::new(0.4, 0.85, 0.3).normalized(),
            flat_floor: false,
        };
        f.clear(bg);
        f
    }

    /// Enable linear distance fog toward `c` between the two depths.
    pub fn set_fog(&mut self, start: f32, end: f32, bg: [u8; 4]) {
        self.fog_start = start;
        self.fog_end = end.max(start + 1.0);
        self.fog_color = [
            bg[0] as f32 / 255.0,
            bg[1] as f32 / 255.0,
            bg[2] as f32 / 255.0,
        ];
        self.fog_on = true;
    }

    pub fn clear(&mut self, bg: [u8; 4]) {
        for px in self.color.chunks_exact_mut(4) {
            px[0] = bg[0];
            px[1] = bg[1];
            px[2] = bg[2];
            px[3] = bg[3];
        }
        self.depth.fill(f32::NEG_INFINITY);
    }

    /// Draw a mesh. `positions` are in mesh space (transformed by `model`).
    /// Optional per-vertex colors; otherwise `fallback` uniform color.
    /// Optional UVs + texture: perspective-correct bilinear sampling,
    /// multiplied over the vertex color (tint).
    ///
    /// R15: when `self.depth_only` the projection is ORTHOGRAPHIC (shadow
    /// map pass — only light-space depth is written); when `self.env_mode`
    /// fragments are shaded by the Egregoria environment (linear albedo²,
    /// analytic sun + shadows, additive atmosphere fog, optional contours,
    /// or the Gerstner water path when `self.water`).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_mesh(
        &mut self,
        positions: &[Vec3],
        indices: &[u32],
        colors: Option<&[[f32; 4]]>,
        uvs: Option<&[[f32; 2]]>,
        tex: Option<&crate::textures::Texture>,
        model: &Mat4,
        view: &Mat4,
        proj: &Mat4,
        eye: Vec3,
        light: Vec3,
        fallback: [f32; 4],
    ) {
        let mv = view.mul(model);
        let near = 0.05f32;

        // empty index list => sequential triangles
        let use_indices = !indices.is_empty();
        let tri_count = if use_indices {
            indices.len() / 3
        } else {
            positions.len() / 3
        };

        for t in 0..tri_count {
            let (ia, ib, ic) = if use_indices {
                (
                    indices[t * 3] as usize,
                    indices[t * 3 + 1] as usize,
                    indices[t * 3 + 2] as usize,
                )
            } else {
                (t * 3, t * 3 + 1, t * 3 + 2)
            };
            if ia >= positions.len() || ib >= positions.len() || ic >= positions.len() {
                continue;
            }
            let wa = model.transform_point(positions[ia]).0;
            let wb = model.transform_point(positions[ib]).0;
            let wc = model.transform_point(positions[ic]).0;

            // view space (before projection)
            let (va, _) = mv.transform_point(positions[ia]);
            let (vb, _) = mv.transform_point(positions[ib]);
            let (vc, _) = mv.transform_point(positions[ic]);

            // near-plane clip in view space (keep z <= -near): Sutherland–Hodgman
            let poly = clip_near(
                [(va, ia, wa), (vb, ib, wb), (vc, ic, wc)],
                near,
            );
            if poly.len() < 3 {
                continue;
            }

            // project all vertices
            let mut scr: Vec<ScrV> = Vec::with_capacity(poly.len());
            for (v, vi, w) in &poly {
                let (px, py, iw) = if self.depth_only {
                    // orthographic (shadow map): no perspective divide
                    let ndc_x = v.x * proj.m[0] + proj.m[3];
                    let ndc_y = v.y * proj.m[5] + proj.m[7];
                    let px = (ndc_x * 0.5 + 0.5) * self.w as f32;
                    let py = (1.0 - (ndc_y * 0.5 + 0.5)) * self.full_h as f32 - self.y_base as f32;
                    (px, py, 1.0)
                } else {
                    let iw = -1.0 / v.z; // v.z negative in front of camera
                    let ndc_x = v.x * iw * proj.m[0];
                    let ndc_y = v.y * iw * proj.m[5];
                    let px = (ndc_x * 0.5 + 0.5) * self.w as f32;
                    let py = (1.0 - (ndc_y * 0.5 + 0.5)) * self.full_h as f32 - self.y_base as f32;
                    (px, py, iw)
                };
                let z = v.z;
                let col = colors
                    .and_then(|cs| cs.get(*vi).copied())
                    .unwrap_or(fallback);
                let uv = uvs.and_then(|us| us.get(*vi).copied()).unwrap_or([0.0, 0.0]);
                scr.push((px, py, z, iw, col, uv, [w.x, w.y, w.z]));
            }

            if self.depth_only {
                // shadow pass: fill depth, nothing else
                for k in 1..scr.len() - 1 {
                    raster_depth_tri(self, scr[0], scr[k], scr[k + 1]);
                }
                continue;
            }

            // flat world-space normal + lambert
            let n = (wb - wa).cross(wc - wa).normalized();
            let centroid = (wa + wb + wc) * (1.0 / 3.0);
            let mut n = n;
            let view_dir = (centroid - eye).normalized();
            if n.dot(view_dir) > 0.0 {
                n = n * -1.0;
            }
            let lam = n.dot(light).max(0.0);
            let mut shade = 0.38 + 0.62 * lam;

            // Blender-style viewport shading overrides
            let mut local_tex = tex;
            match self.shade {
                ShadeMode::Normals => {
                    let nc = [
                        (n.x * 0.5 + 0.5).clamp(0.0, 1.0),
                        (n.y * 0.5 + 0.5).clamp(0.0, 1.0),
                        (n.z * 0.5 + 0.5).clamp(0.0, 1.0),
                        1.0,
                    ];
                    for s in scr.iter_mut() {
                        s.4 = nc;
                    }
                    shade = 1.0;
                    local_tex = None;
                }
                ShadeMode::Wireframe => {
                    // dark depth-writing fill (hidden-line) — edges after
                    for s in scr.iter_mut() {
                        s.4 = [0.16, 0.16, 0.19, 1.0];
                    }
                    shade = 1.0;
                    local_tex = None;
                }
                ShadeMode::Depth => {
                    // grayscale view-depth: white near → black far
                    let (near, far) = self.depth_range;
                    let span = (far - near).max(1.0);
                    for s in scr.iter_mut() {
                        let d = (-s.2 - near) / span; // s.2 = view z (negative)
                        let g = (1.0 - d.clamp(0.0, 1.0)) as f32;
                        s.4 = [g, g, g, 1.0];
                    }
                    shade = 1.0;
                    local_tex = None;
                }
                ShadeMode::ShadedWire | ShadeMode::Solid | ShadeMode::Quality => {}
            }

            // fan triangulate the clipped polygon
            for k in 1..scr.len() - 1 {
                let p0 = scr[0];
                let p1 = scr[k];
                let p2 = scr[k + 1];
                raster_tri(self, p0, p1, p2, shade, local_tex);
            }

            // wireframe edges on top of the depth fill
            if self.shade == ShadeMode::Wireframe || self.shade == ShadeMode::ShadedWire {
                let pts: Vec<(f32, f32, f32)> =
                    scr.iter().map(|s| (s.0, s.1, s.2)).collect();
                let edge = if self.shade == ShadeMode::Wireframe {
                    [0.62, 0.78, 0.95, 1.0]
                } else {
                    // dark edges over the solid fill (Blender "curvature
                    // overlay" look — raster_line writes opaque)
                    [0.13, 0.13, 0.16, 1.0]
                };
                for w in pts.windows(2) {
                    raster_line(self, w[0], w[1], edge);
                }
                if pts.len() > 2 {
                    raster_line(self, *pts.last().unwrap(), pts[0], edge);
                }
            }
        }
    }

    /// DDA line with depth test/write (wireframe edges).
    pub fn draw_line_px(&mut self, x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32, c: [f32; 4]) {
        raster_line(self, (x0, y0, z0), (x1, y1, z1), c);
    }
}

fn raster_line(f: &mut Frame, a: (f32, f32, f32), b: (f32, f32, f32), c: [f32; 4]) {
    let (x0, y0, z0) = a;
    let (x1, y1, z1) = b;
    let steps = ((x1 - x0).abs().max((y1 - y0).abs()).ceil() as usize).max(1);
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let x = x0 + (x1 - x0) * t;
        let y = y0 + (y1 - y0) * t;
        let z = z0 + (z1 - z0) * t;
        let xi = x as isize;
        let yi = y as isize + f.y_base as isize;
        if xi < 0 || yi < 0 || xi >= f.w as isize || yi >= f.full_h as isize {
            continue;
        }
        let idx = (yi as usize) * f.w + xi as usize;
        if idx >= f.depth.len() {
            continue;
        }
        if z >= f.depth[idx] {
            f.depth[idx] = z;
            let o = idx * 4;
            f.color[o] = (c[0] * 255.0) as u8;
            f.color[o + 1] = (c[1] * 255.0) as u8;
            f.color[o + 2] = (c[2] * 255.0) as u8;
            f.color[o + 3] = 255;
        }
    }
}

type VV = (Vec3, usize, Vec3);

fn clip_near(poly: [VV; 3], near: f32) -> Vec<VV> {
    // plane: z + near = 0, keep z <= -near  <=>  f = z + near <= 0
    let f = |v: &VV| v.0.z + near;
    let mut out: Vec<VV> = Vec::with_capacity(4);
    for i in 0..3 {
        let cur = &poly[i];
        let nxt = &poly[(i + 1) % 3];
        let fc = f(cur);
        let fn_ = f(nxt);
        if fc <= 0.0 {
            out.push(*cur);
        }
        if (fc < 0.0 && fn_ > 0.0) || (fc > 0.0 && fn_ < 0.0) {
            let t = fc / (fc - fn_);
            let p = Vec3::new(
                cur.0.x + (nxt.0.x - cur.0.x) * t,
                cur.0.y + (nxt.0.y - cur.0.y) * t,
                cur.0.z + (nxt.0.z - cur.0.z) * t,
            );
            let pw = Vec3::new(
                cur.2.x + (nxt.2.x - cur.2.x) * t,
                cur.2.y + (nxt.2.y - cur.2.y) * t,
                cur.2.z + (nxt.2.z - cur.2.z) * t,
            );
            out.push((p, cur.1, pw)); // attributes from the visible-side vertex
        }
    }
    out
}

/// Screen vertex: (px, py, view-z, iw, rgba, uv, world xyz)
type ScrV = (f32, f32, f32, f32, [f32; 4], [f32; 2], [f32; 3]);

/// Shadow-map triangle fill: light-space depth only.
fn raster_depth_tri(f: &mut Frame, p0: ScrV, p1: ScrV, p2: ScrV) {
    let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
    if area.abs() < 1e-6 {
        return;
    }
    let inv_area = 1.0 / area;
    let min_x = p0.0.min(p1.0).min(p2.0).floor().max(0.0) as i64;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil().min(f.w as f32 - 1.0) as i64;
    let min_y = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as i64;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil().min(f.h as f32 - 1.0) as i64;
    if min_x > max_x || min_y > max_y {
        return;
    }
    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let (fx, fy) = (px as f64, py as f64);
            let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, fx, fy);
            let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, fx, fy);
            let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, fx, fy);
            let (b0, b1, b2);
            if area > 0.0 {
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                b0 = w0 * inv_area;
                b1 = w1 * inv_area;
                b2 = w2 * inv_area;
            } else {
                if w0 > 0.0 || w1 > 0.0 || w2 > 0.0 {
                    continue;
                }
                b0 = w0 * inv_area;
                b1 = w1 * inv_area;
                b2 = w2 * inv_area;
            }
            let z = b0 * p0.2 + b1 * p1.2 + b2 * p2.2;
            let idx = (py as usize) * f.w + px as usize;
            if z > f.depth[idx] {
                f.depth[idx] = z;
            }
        }
    }
}

fn raster_tri(
    f: &mut Frame,
    p0: ScrV,
    p1: ScrV,
    p2: ScrV,
    shade: f32,
    tex: Option<&crate::textures::Texture>,
) {
    let area = (p1.0 - p0.0) * (p2.1 - p0.1) - (p1.1 - p0.1) * (p2.0 - p0.0);
    if area.abs() < 1e-6 {
        return;
    }
    let inv_area = 1.0 / area;
    let min_x = p0.0.min(p1.0).min(p2.0).floor().max(0.0) as i64;
    let max_x = p0.0.max(p1.0).max(p2.0).ceil().min(f.w as f32 - 1.0) as i64;
    let min_y = p0.1.min(p1.1).min(p2.1).floor().max(0.0) as i64;
    let max_y = p0.1.max(p1.1).max(p2.1).ceil().min(f.h as f32 - 1.0) as i64;
    if min_x > max_x || min_y > max_y {
        return;
    }

    for py in min_y..=max_y {
        for px in min_x..=max_x {
            let (fx, fy) = (px as f64, py as f64);
            let w0 = edge_fn(p1.0, p1.1, p2.0, p2.1, fx, fy);
            let w1 = edge_fn(p2.0, p2.1, p0.0, p0.1, fx, fy);
            let w2 = edge_fn(p0.0, p0.1, p1.0, p1.1, fx, fy);
            // two-sided: all barycentrics must share the sign of `area`
            let (b0, b1, b2);
            if area > 0.0 {
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                b0 = w0 * inv_area;
                b1 = w1 * inv_area;
                b2 = w2 * inv_area;
            } else {
                if w0 > 0.0 || w1 > 0.0 || w2 > 0.0 {
                    continue;
                }
                b0 = w0 * inv_area;
                b1 = w1 * inv_area;
                b2 = w2 * inv_area;
            }
            // perspective-correct depth: z = (Σ b_i * z_i * iw_i) / (Σ b_i * iw_i)
            let iw = b0 * p0.3 + b1 * p1.3 + b2 * p2.3;
            if iw <= 1e-9 {
                continue;
            }
            let ziw = b0 * p0.2 * p0.3 + b1 * p1.2 * p1.3 + b2 * p2.2 * p2.3;
            let z = ziw / iw;
            let idx = (py as usize) * f.w + px as usize;
            if z > f.depth[idx] {
                // R15 Egregoria environment path: linear HDR shading.
                if f.env_mode {
                    env_fragment(f, idx, z, iw, &p0, &p1, &p2, b0, b1, b2, area, tex);
                    continue;
                }
                f.depth[idx] = z;
                // color: perspective-correct vertex color * lambert shade
                let mut cr = (b0 * p0.4[0] * p0.3 + b1 * p1.4[0] * p1.3 + b2 * p2.4[0] * p2.3) / iw * shade;
                let mut cg = (b0 * p0.4[1] * p0.3 + b1 * p1.4[1] * p1.3 + b2 * p2.4[1] * p2.3) / iw * shade;
                let mut cb = (b0 * p0.4[2] * p0.3 + b1 * p1.4[2] * p1.3 + b2 * p2.4[2] * p2.3) / iw * shade;
                if let Some(t) = tex {
                    // perspective-correct UV sampling; the mip level is chosen
                    // PER TRIANGLE from the texel-to-pixel footprint so distant
                    // surfaces read averaged texels (no shimmer, no dark mush)
                    let uu = (b0 * p0.5[0] * p0.3 + b1 * p1.5[0] * p1.3 + b2 * p2.5[0] * p2.3) / iw;
                    let vv = (b0 * p0.5[1] * p0.3 + b1 * p1.5[1] * p1.3 + b2 * p2.5[1] * p2.3) / iw;
                    let s = sample_tri_lod(t, &p0, &p1, &p2, area, uu, vv);
                    cr *= s[0];
                    cg *= s[1];
                    cb *= s[2];
                }
                if f.fog_on {
                    let dist = -z;
                    let t = ((dist - f.fog_start) / (f.fog_end - f.fog_start)).clamp(0.0, 1.0);
                    cr = cr + (f.fog_color[0] - cr) * t;
                    cg = cg + (f.fog_color[1] - cg) * t;
                    cb = cb + (f.fog_color[2] - cb) * t;
                }
                let o = idx * 4;
                f.color[o] = (cr * 255.0).clamp(0.0, 255.0) as u8;
                f.color[o + 1] = (cg * 255.0).clamp(0.0, 255.0) as u8;
                f.color[o + 2] = (cb * 255.0).clamp(0.0, 255.0) as u8;
                f.color[o + 3] = 255;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn env_fragment(
    f: &mut Frame,
    idx: usize,
    z: f32,
    iw: f32,
    p0: &ScrV,
    p1: &ScrV,
    p2: &ScrV,
    b0: f32,
    b1: f32,
    b2: f32,
    area: f32,
    tex: Option<&crate::textures::Texture>,
) {
    // perspective-correct world position
    let wx = (b0 * p0.6[0] * p0.3 + b1 * p1.6[0] * p1.3 + b2 * p2.6[0] * p2.3) / iw;
    let wy = (b0 * p0.6[1] * p0.3 + b1 * p1.6[1] * p1.3 + b2 * p2.6[1] * p2.3) / iw;
    let wz = (b0 * p0.6[2] * p0.3 + b1 * p1.6[2] * p1.3 + b2 * p2.6[2] * p2.3) / iw;
    let dist = -z;
    let dir = Vec3::new(wx - f.eye.x, wy - f.eye.y, wz - f.eye.z).normalized();
    let wpos = Vec3::new(wx, wy, wz);

    // surface normal: recompute flat normal from world verts (cheap, exact)
    let e1 = Vec3::new(p1.6[0] - p0.6[0], p1.6[1] - p0.6[1], p1.6[2] - p0.6[2]);
    let e2 = Vec3::new(p2.6[0] - p0.6[0], p2.6[1] - p0.6[1], p2.6[2] - p0.6[2]);
    let mut n = e1.cross(e2).normalized();
    if n.dot(dir) > 0.0 {
        n = n * -1.0; // face the camera
    }

    // albedo (sRGB 0..1) -> linear via the gamma-2.0 square approximation
    let mut al = if f.water {
        // water ignores albedo: gerstner + sky reflection below
        [0.0, 0.0, 0.0]
    } else {
        let mut cr = (b0 * p0.4[0] * p0.3 + b1 * p1.4[0] * p1.3 + b2 * p2.4[0] * p2.3) / iw;
        let mut cg = (b0 * p0.4[1] * p0.3 + b1 * p1.4[1] * p1.3 + b2 * p2.4[1] * p2.3) / iw;
        let mut cb = (b0 * p0.4[2] * p0.3 + b1 * p1.4[2] * p1.3 + b2 * p2.4[2] * p2.3) / iw;
        if let Some(t) = tex {
            let uu = (b0 * p0.5[0] * p0.3 + b1 * p1.5[0] * p1.3 + b2 * p2.5[0] * p2.3) / iw;
            let vv = (b0 * p0.5[1] * p0.3 + b1 * p1.5[1] * p1.3 + b2 * p2.5[1] * p2.3) / iw;
            let s = sample_tri_lod(t, p0, p1, p2, area, uu, vv);
            cr *= s[0];
            cg *= s[1];
            cb *= s[2];
        }
        [cr * cr, cg * cg, cb * cb]
    };

    // EDITOR floor: unlit, exactly the backdrop color (linear). The compose
    // pass applies the same autoexposure+tonemap to it as to the flat sky,
    // so floor and backdrop encode to the identical paper color.
    if f.flat_floor {
        f.depth[idx] = z;
        let o = idx * 3;
        f.hdr[o] = al[0];
        f.hdr[o + 1] = al[1];
        f.hdr[o + 2] = al[2];
        return;
    }

    // analytic sun (engine::sun): ambient + sun_col * max(n·l) * shadow.
    // Shadows are SOFTENED (0.35 floor) like Egregoria's ambient/IBL keeps
    // shadowed ground lit by the sky — never pure black.
    let nl = n.dot(light_dir(f)).max(0.0);
    let raw = f
        .shadow
        .as_ref()
        .map(|sm| sm.test(wpos, nl, true))
        .unwrap_or(1.0);
    let shadow = 0.35 + 0.65 * raw;
    let mut lum = [
        0.05 + f.sun_col[0] * nl * shadow,
        0.05 + f.sun_col[1] * nl * shadow,
        0.05 + f.sun_col[2] * nl * shadow,
    ];

    // elevation contour lines on the ground (Egregoria terraforming bands,
    // made always-on: 10 m thin + 50 m thin black lines with fwidth fade)
    if f.contours && f.ground_pass {
        let screen_area = (area.abs() * 0.5).max(0.25);
        let dw1 = (p1.6[1] - p0.6[1]).abs();
        let dw2 = (p2.6[1] - p0.6[1]).abs();
        // world-height gradient per screen pixel (fwidth substitute)
        let fw = (((dw1 + dw2) / screen_area.sqrt()) * 2.5).max(0.004);
        let line = |spacing: f32, half: f32, wy: f32| -> f32 {
            let m = ((wy % spacing) + spacing) % spacing - half;
            let x = m.abs();
            if x >= fw {
                0.0
            } else {
                1.0 - x / fw // linear fade ≈ smoothstep(fw, 0, x)
            }
        };
        let a4 = line(10.0, 5.0, wy) * 0.10;
        let a5 = line(50.0, 25.0, wy) * 0.10;
        let aa = (a4 + a5).min(0.2);
        for v in al.iter_mut() {
            *v *= 1.0 - aa;
        }
    }

    if f.water {
        // Gerstner wave normal + TEX_WAVY perturbation (water.frag.wgsl)
        let mut wn = gerstner_wave_normal_unit((wx, wz), f.env_time);
        if let Some(t) = tex {
            let tu = wx * 0.001 + f.env_time * 0.02;
            let tv = wz * 0.001 - f.env_time * 0.01;
            let s = t.sample_lod(tu, tv, 0);
            wn.0 += (s[0] * 2.0 - 1.0) * 0.15;
            wn.1 -= (s[1] * 2.0 - 1.0) * 0.12;
            wn.2 += (s[2] * 2.0 - 1.0) * 0.10;
            let l = (wn.0 * wn.0 + wn.1 * wn.1 + wn.2 * wn.2).sqrt().max(1e-5);
            wn = (wn.0 / l, wn.1 / l, wn.2 / l);
        }
        let nrm = Vec3::new(wn.0, wn.1, wn.2);
        // reflected view ray (V = eye->frag; reflect around the wave normal)
        let d = dir;
        let dd = d.dot(nrm);
        let r = Vec3::new(d.x - 2.0 * dd * nrm.x, d.y - 2.0 * dd * nrm.y, d.z - 2.0 * dd * nrm.z);
        let r_up = Vec3::new(r.x, r.y.max(0.03), r.z).normalized();
        let sky_refl = f
            .sky
            .as_ref()
            .map(|s| s.sample(r_up))
            .unwrap_or([0.3, 0.45, 0.6]);
        // view vector toward the camera (for the fresnel-ish reflect coeff)
        let to_cam = Vec3::new(-d.x, -d.y, -d.z);
        let rc = engine::water::water_reflect_coeff(to_cam, wn);
        // shore fade: water column depth from the depth buffer BEHIND this
        // fragment (terrain distance minus water distance, projected down)
        let terrain_dist = -f.depth[idx]; // NEG_INF over open ground rim → treated deep
        let vert = if terrain_dist.is_finite() && terrain_dist > dist {
            (terrain_dist - dist) * (0.0 - d.y).max(0.2)
        } else {
            12.0
        };
        let bf = shore_factor(vert.clamp(0.0, 64.0));
        al = [
            bf * WATER_BASE[0] + 0.1 * rc * sky_refl[0],
            bf * WATER_BASE[1] + 0.1 * rc * sky_refl[1],
            bf * WATER_BASE[2] + 0.1 * rc * sky_refl[2],
        ];
        // still sun-lit (specular-free, matching the reference fragment)
        lum = [0.25 + 0.75 * f.sun_col[0] * 0.4, 0.25 + 0.75 * f.sun_col[1] * 0.4, 0.25 + 0.75 * f.sun_col[2] * 0.4];
    }

    // Egregoria combines `ambient + Lo + fog` in linear space, one tonemap
    let mut c = [al[0] * lum[0], al[1] * lum[1], al[2] * lum[2]];
    // ROADKIT_NOFOG=1: skip the aerial perspective (diagnostics; read once)
    static NOFOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let nofog = *NOFOG.get_or_init(|| std::env::var("ROADKIT_NOFOG").is_ok());
    if !nofog {
        if let Some(fog) = &f.fog_lut {
            let fc = fog.sample(dir, dist);
            c[0] += fc[0];
            c[1] += fc[1];
            c[2] += fc[2];
        }
    }
    f.depth[idx] = z;
    let o = idx * 3;
    f.hdr[o] = c[0];
    f.hdr[o + 1] = c[1];
    f.hdr[o + 2] = c[2];
}

/// The normalized light (sun) direction carried on the frame.
#[inline]
fn light_dir(f: &Frame) -> Vec3 {
    f.light
}

/// Perspective-correct edge helper (signed edge function).
fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f64, py: f64) -> f32 {
    ((bx - ax) as f64 * (py - ay as f64) - (by - ay) as f64 * (px - ax as f64)) as f32
}

/// Per-triangle mip selection: compare the triangle's UV footprint (in
/// texels) against its screen area (in pixels); `log2` of that ratio is
/// the mip level where one texel ≈ one pixel. The FRACTIONAL level is
/// blended between the two nearest mips (trilinear) — on large ground
/// triangles the discrete per-triangle level jumps used to show as radial
/// mip-boundary streaks; the frac blend removes them.
fn sample_tri_lod(
    t: &crate::textures::Texture,
    p0: &ScrV,
    p1: &ScrV,
    p2: &ScrV,
    screen_area2: f32,
    uu: f32,
    vv: f32,
) -> [f32; 4] {
    let du1 = p1.5[0] - p0.5[0];
    let dv1 = p1.5[1] - p0.5[1];
    let du2 = p2.5[0] - p0.5[0];
    let dv2 = p2.5[1] - p0.5[1];
    let uv_area2 = (du1 * dv2 - du2 * dv1).abs();
    let texel_area = uv_area2 * 0.5 * (t.w.max(1) * t.h.max(1)) as f32;
    let screen_area = (screen_area2.abs() * 0.5).max(0.25);
    let texels_per_px = (texel_area / screen_area).sqrt();
    if texels_per_px <= 1.0 {
        return t.sample_lod(uu, vv, 0);
    }
    let level = texels_per_px.log2().clamp(0.0, t.mips.len() as f32 - 1.0);
    let l0 = level.floor() as usize;
    let l1 = (l0 + 1).min(t.mips.len() - 1);
    let frac = level - l0 as f32;
    if frac < 0.01 || l0 == l1 {
        return t.sample_lod(uu, vv, l0);
    }
    let a = t.sample_lod(uu, vv, l0);
    let b = t.sample_lod(uu, vv, l1);
    [
        a[0] + (b[0] - a[0]) * frac,
        a[1] + (b[1] - a[1]) * frac,
        a[2] + (b[2] - a[2]) * frac,
        a[3] + (b[3] - a[3]) * frac,
    ]
}

/// Box-filter downsample an RGBA8 buffer by an integer factor (Quality
/// mode SSAA: render at `w*ss × h*ss`, shrink back to w×h).
pub fn box_downsample(rgba: &[u8], w: usize, h: usize, ss: usize) -> Vec<u8> {
    if ss <= 1 {
        return rgba.to_vec();
    }
    let ow = w / ss;
    let oh = h / ss;
    let mut out = vec![0u8; ow * oh * 4];
    let ss2 = (ss * ss) as u32;
    for oy in 0..oh {
        for ox in 0..ow {
            let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
            for dy in 0..ss {
                for dx in 0..ss {
                    let i = ((oy * ss + dy) * w + (ox * ss + dx)) * 4;
                    r += rgba[i] as u32;
                    g += rgba[i + 1] as u32;
                    b += rgba[i + 2] as u32;
                }
            }
            let o = (oy * ow + ox) * 4;
            out[o] = (r / ss2) as u8;
            out[o + 1] = (g / ss2) as u8;
            out[o + 2] = (b / ss2) as u8;
            out[o + 3] = 255;
        }
    }
    out
}

/// Box-filter downsample a linear HDR rgb3 buffer (env Quality SSAA).
pub fn box_downsample_f32(hdr: &[f32], depth: &[f32], w: usize, h: usize, ss: usize) -> (Vec<f32>, Vec<f32>) {
    if ss <= 1 {
        return (hdr.to_vec(), depth.to_vec());
    }
    let ow = w / ss;
    let oh = h / ss;
    let mut out = vec![0f32; ow * oh * 3];
    let mut out_d = vec![f32::NEG_INFINITY; ow * oh];
    let ssf = (ss * ss) as f32;
    for oy in 0..oh {
        for ox in 0..ow {
            let (mut r, mut g, mut b) = (0f32, 0f32, 0f32);
            let mut zd = f32::NEG_INFINITY;
            for dy in 0..ss {
                for dx in 0..ss {
                    let i = (oy * ss + dy) * w + (ox * ss + dx);
                    let o3 = i * 3;
                    r += hdr[o3];
                    g += hdr[o3 + 1];
                    b += hdr[o3 + 2];
                    // nearest (max) depth wins — keeps sky/geometry split exact
                    zd = zd.max(depth[i]);
                }
            }
            let o = (oy * ow + ox) * 3;
            out[o] = r / ssf;
            out[o + 1] = g / ssf;
            out[o + 2] = b / ssf;
            out_d[oy * ow + ox] = zd;
        }
    }
    (out, out_d)
}

/// Everything `compose_env` needs to finish a frame (Egregoria-style:
/// sky fill from the LUT + analytic sun disc, autoexposure, tonemap,
/// dither, gamma-2.0 encode).
pub struct EnvCompose<'a> {
    pub eye: Vec3,
    /// world→view matrix (for rotating screen rays back to world space)
    pub view: &'a Mat4,
    /// vertical FOV in radians
    pub fov: f32,
    pub w: usize,
    pub h: usize,
    /// normalized sun direction (y up)
    pub sun: Vec3,
    pub sky: &'a SkyLut,
    /// EDITOR look: flat linear backdrop for sky pixels (no LUT, no sun
    /// disc). None = the full Egregoria atmosphere.
    pub flat_bg: Option<[f32; 3]>,
}

/// Final Egregoria compose over a linear HDR buffer + depth:
/// unwritten depth = sky (atmosphere LUT + sun disc); then the shared
/// autoexposure → tonemap → dither → gamma-2.0 encode pipeline.
pub fn compose_env(hdr: &mut [f32], depth: &[f32], p: &EnvCompose) -> Vec<u8> {
    let (w, h) = (p.w, p.h);
    let tan_v = (p.fov * 0.5).tan();
    let tan_h = tan_v * (w as f32 / h.max(1) as f32);
    let auto = engine::atmosphere::autoexposure(p.sun.y);
    let mut out = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let o3 = i * 3;
            if depth[i] == f32::NEG_INFINITY {
                // screen ray -> world direction (view rotation is orthonormal)
                let ndc_x = (((x as f32 + 0.5) / w as f32) * 2.0 - 1.0) * tan_h;
                let ndc_y = (1.0 - ((y as f32 + 0.5) / h as f32) * 2.0) * tan_v;
                let (dx, dy, dz) = (ndc_x, ndc_y, -1.0);
                let vx = p.view.m[0] * dx + p.view.m[4] * dy + p.view.m[8] * dz;
                let vy = p.view.m[1] * dx + p.view.m[5] * dy + p.view.m[9] * dz;
                let vz = p.view.m[2] * dx + p.view.m[6] * dy + p.view.m[10] * dz;
                let dir = Vec3::new(vx, vy, vz).normalized();
                let c = match p.flat_bg {
                    // EDITOR look: flat paper — no sky LUT, no sun disc
                    Some(bg) => bg,
                    None => {
                        let mut c = p.sky.sample(dir);
                        let disc = engine::atmosphere::sun_disc(p.sun, dir);
                        c[0] += disc;
                        c[1] += disc;
                        c[2] += disc;
                        c
                    }
                };
                hdr[o3] = c[0];
                hdr[o3 + 1] = c[1];
                hdr[o3 + 2] = c[2];
            }
            // autoexposure -> tonemap -> dither -> gamma-2.0 encode
            let mut c = [
                hdr[o3] * auto,
                hdr[o3 + 1] * auto,
                hdr[o3 + 2] * auto,
            ];
            c = engine::atmosphere::tonemap(c);
            let d = engine::atmosphere::dither(x as u32, y as u32);
            for v in c.iter_mut() {
                *v = (*v + d).clamp(0.0, 1.0).sqrt();
            }
            let o = i * 4;
            out[o] = (c[0] * 255.0) as u8;
            out[o + 1] = (c[1] * 255.0) as u8;
            out[o + 2] = (c[2] * 255.0) as u8;
            out[o + 3] = 255;
        }
    }
    out
}
