//! Tiny CPU software rasterizer (z-buffered, flat-shaded, vertex colors).
//! Renders the 3D road scene + GLB props to RGBA pixels that the egui UI
//! uploads as a texture. No GPU dependency — works everywhere eframe runs.
//!
//! R10 adds Blender-style viewport shading: Solid (default), Normals
//! (world-normal RGB) and Wireframe (hidden-line: dark depth fill +
//! bright edges) — see `ShadeMode`.

use gdmath::mat4::Mat4;
use gdmath::vec3::Vec3;

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
            let poly = clip_near([(va, ia), (vb, ib), (vc, ic)], near);
            if poly.len() < 3 {
                continue;
            }

            // project all vertices
            let mut scr: Vec<(f32, f32, f32, f32, [f32; 4], [f32; 2])> = Vec::with_capacity(poly.len());
            for (v, vi) in &poly {
                let iw = -1.0 / v.z; // v.z negative in front of camera
                let ndc_x = v.x * iw * proj.m[0];
                let ndc_y = v.y * iw * proj.m[5];
                let px = (ndc_x * 0.5 + 0.5) * self.w as f32;
                let py = (1.0 - (ndc_y * 0.5 + 0.5)) * self.full_h as f32 - self.y_base as f32;
                let z = v.z;
                let col = colors
                    .and_then(|cs| cs.get(*vi).copied())
                    .unwrap_or(fallback);
                let uv = uvs.and_then(|us| us.get(*vi).copied()).unwrap_or([0.0, 0.0]);
                scr.push((px, py, z, iw, col, uv));
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

type VV = (Vec3, usize);

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
            out.push((p, cur.1)); // attribute from the visible-side vertex
        }
    }
    out
}

type ScrV = (f32, f32, f32, f32, [f32; 4], [f32; 2]);

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
                f.depth[idx] = z;
                // color: perspective-correct vertex color * lambert shade
                let mut cr = (b0 * p0.4[0] * p0.3 + b1 * p1.4[0] * p1.3 + b2 * p2.4[0] * p2.3) / iw * shade;
                let mut cg = (b0 * p0.4[1] * p0.3 + b1 * p1.4[1] * p1.3 + b2 * p2.4[1] * p2.3) / iw * shade;
                let mut cb = (b0 * p0.4[2] * p0.3 + b1 * p1.4[2] * p1.3 + b2 * p2.4[2] * p2.3) / iw * shade;
                if let Some(t) = tex {
                    // perspective-correct UV sampling (bilinear, wrap)
                    let uu = (b0 * p0.5[0] * p0.3 + b1 * p1.5[0] * p1.3 + b2 * p2.5[0] * p2.3) / iw;
                    let vv = (b0 * p0.5[1] * p0.3 + b1 * p1.5[1] * p1.3 + b2 * p2.5[1] * p2.3) / iw;
                    let s = t.sample(uu, vv);
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

/// Perspective-correct edge helper (signed edge function).
fn edge_fn(ax: f32, ay: f32, bx: f32, by: f32, px: f64, py: f64) -> f32 {
    ((bx - ax) as f64 * (py - ay as f64) - (by - ay) as f64 * (px - ax as f64)) as f32
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
