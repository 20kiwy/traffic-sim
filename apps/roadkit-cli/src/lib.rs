//! roadkit-cli — headless RoadKit: run a .gd scenario, render 2D plan + 3D view to PNG.
//!
//! Uses the exact same script bridge as the GUI (`egui_app::bridge`), the same
//! procedural mesh builder and the same software rasterizer — the CLI is how
//! `cargo xtask dist` screenshots every scenario per version.

use gdmath::{OrbitCam, Vec3};
use roadkit::{build_scene_with_bg, render_scene_env, AssetLibrary, TextureLibrary};
use roadkit::bridge_host::{self, SceneState};
use std::cell::RefCell;
use std::rc::Rc;

/// Resolve a script name to a path: bare names resolve against assets/scripts and
/// assets/scenarios (searched from cwd upward, then the repo root).
pub fn resolve_script(name: &str) -> Result<std::path::PathBuf, String> {
    let p = std::path::Path::new(name);
    if p.exists() {
        return Ok(p.to_path_buf());
    }
    let mut cur = std::env::current_dir().map_err(|e| e.to_string())?;
    for _ in 0..6 {
        for base in ["assets/scenarios", "assets/scripts"] {
            let c = cur.join(base).join(format!("{name}.gd"));
            if c.exists() {
                return Ok(c);
            }
        }
        if !cur.pop() {
            break;
        }
    }
    Err(format!("script `{name}` not found (tried path, assets/scenarios, assets/scripts)"))
}

/// A fully built world: script output + junctions + assets + textures.
pub struct World {
    pub scene: SceneState,
    pub library: AssetLibrary,
    pub textures: TextureLibrary,
}

impl World {
    pub fn log_lines(&self) -> impl Iterator<Item = &String> {
        self.scene.console.iter()
    }
}

/// Run a .gd script and collect the world (auto-join + junction detection included).
pub fn build_world(src: &str, assets_dir: &std::path::Path) -> Result<World, String> {
    let script = gdscript::parse(src)?;
    let scene: Rc<RefCell<SceneState>> = Rc::new(RefCell::new(SceneState::default()));
    let mut it = gdscript::Interpreter::new();
    bridge_host::register(&mut it, &scene);
    it.run(&script, "main")?;
    let mut st = scene.borrow_mut();
    for l in it.take_output() {
        st.console.push(l);
    }
    st.auto_join();
    Ok(World {
        library: AssetLibrary::load_dir(assets_dir),
        textures: TextureLibrary::load_dir(&assets_dir.join("textures")),
        scene: std::mem::take(&mut *st),
    })
}

/// Build the world from a script name, walking up to the repo root for assets/.
pub fn build_world_named(script_name: &str) -> Result<(World, std::path::PathBuf), String> {
    let path = resolve_script(script_name)?;
    let src = std::fs::read_to_string(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
    let assets = find_assets_dir(&path)?;
    Ok((build_world(&src, &assets)?, path))
}

fn find_assets_dir(script_path: &std::path::Path) -> Result<std::path::PathBuf, String> {
    let mut cur = script_path.ancestors().find(|a| a.join("assets/glb").exists()).map(|a| a.join("assets"));
    if cur.is_none() {
        let mut c = std::env::current_dir().map_err(|e| e.to_string())?;
        for _ in 0..6 {
            if c.join("assets/glb").exists() {
                cur = Some(c.join("assets"));
                break;
            }
            if !c.pop() {
                break;
            }
        }
    }
    cur.ok_or_else(|| "assets dir (assets/glb) not found".into())
}

/// Render one view ("plan" | "3d") to RGBA8. `ss` = supersample factor
/// (render at ss× resolution, box-downsample — smooth antialiased edges).
pub fn render_view(world: &World, view: &str, zoom: f32, w: usize, h: usize, ss: usize) -> Result<Vec<u8>, String> {
    render_view_opt(world, view, zoom, w, h, ss, None)
}

/// Full-control variant: `focus` = Some((x, z, radius_m)) frames a sub-area
/// (detail crops of junctions, stations, props).
pub fn render_view_opt(
    world: &World,
    view: &str,
    zoom: f32,
    w: usize,
    h: usize,
    ss: usize,
    focus: Option<(f64, f64, f64)>,
) -> Result<Vec<u8>, String> {
    render_view_sim(world, view, zoom, w, h, ss, focus, 0.0, 14.0)
}

/// Same + traffic simulation: runs `sim_seconds` of deterministic traffic
/// before rendering (vehicles queued at red signals show it working).
#[allow(clippy::too_many_arguments)]
/// Viewport shading override for camera snapshots: ROADKIT_SHADE env
/// (solid | normals | wire | shadedwire | depth | quality) — the Blender
/// style modes from docs/ROADARCHITECT.md §10.
pub fn shade_from_env() -> roadkit::raster::ShadeMode {
    match std::env::var("ROADKIT_SHADE").unwrap_or_default().to_lowercase().as_str() {
        "normals" => roadkit::raster::ShadeMode::Normals,
        "wire" | "wireframe" => roadkit::raster::ShadeMode::Wireframe,
        "shadedwire" | "shaded_wire" | "shaded+w" => roadkit::raster::ShadeMode::ShadedWire,
        "depth" => roadkit::raster::ShadeMode::Depth,
        "quality" => roadkit::raster::ShadeMode::Quality,
        _ => roadkit::raster::ShadeMode::Solid,
    }
}

pub fn render_view_sim(
    world: &World,
    view: &str,
    zoom: f32,
    w: usize,
    h: usize,
    ss: usize,
    focus: Option<(f64, f64, f64)>,
    sim_seconds: f64,
    density: f64,
) -> Result<Vec<u8>, String> {
    match view {
        "3d" => {
            let cam = frame_cam_focused(&world.scene.curves, w, h, focus);
            let mut props = world.scene.props.clone();
            if sim_seconds > 0.0 {
                let mut sim = roadkit::TrafficSim::new(
                    &world.scene.curves,
                    &world.scene.junctions,
                    roadkit::SimSettings { density, ..Default::default() },
                );
                let steps = (sim_seconds / 0.1).ceil() as usize;
                for _ in 0..steps {
                    sim.step(0.1, &world.scene.curves);
                }
                props.extend(sim.prop_instances(&world.scene.curves));
            }
            let scene = build_scene_with_bg(&world.scene.curves, &world.scene.junctions, &props, &world.library, true, roadkit::ENV_DAY.clear);
            let env = roadkit::scene::Environment::from_env();
            if ss <= 1 {
                return Ok(roadkit::scene::render_env_shaded(&scene, &world.library, &world.textures, &cam, w, h, &env, &[], true, shade_from_env()));
            }
            let big = roadkit::scene::render_env_shaded(
                &scene,
                &world.library,
                &world.textures,
                &cam,
                w * ss,
                h * ss,
                &env,
                &[],
                true,
                roadkit::raster::ShadeMode::Solid,
            );
            Ok(box_downsample(&big, w * ss, h * ss, ss))
        }
        "plan" => Ok(plan_render(world, zoom, w, h)),
        _ => Err(format!("unknown view `{view}` (plan|3d)")),
    }
}

/// Box-filter downsample an RGBA8 buffer by an integer factor.
fn box_downsample(src: &[u8], sw: usize, sh: usize, f: usize) -> Vec<u8> {
    let (dw, dh) = (sw / f, sh / f);
    let mut out = vec![0u8; dw * dh * 4];
    let inv = 1.0 / (f * f) as f32;
    for dy in 0..dh {
        for dx in 0..dw {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..f {
                let row = (dy * f + sy) * sw;
                for sx in 0..f {
                    let i = (row + dx * f + sx) * 4;
                    r += src[i] as f32;
                    g += src[i + 1] as f32;
                    b += src[i + 2] as f32;
                    a += src[i + 3] as f32;
                }
            }
            let o = (dy * dw + dx) * 4;
            out[o] = (r * inv) as u8;
            out[o + 1] = (g * inv) as u8;
            out[o + 2] = (b * inv) as u8;
            out[o + 3] = (a * inv) as u8;
        }
    }
    out
}

/// Frame the whole scene (or a sub-area when `focus` is Some((x, z, radius_m)))
/// in the orbit camera (hero 3/4 angle). Computes the exact bbox, rotates it
/// into the camera frame and fits both screen axes so roads fill the frame.
pub fn frame_cam_focused(
    curves: &[roads::model::RoadCurve],
    w: usize,
    h: usize,
    focus: Option<(f64, f64, f64)>,
) -> OrbitCam {
    let (mut min_x, mut min_z) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in curves {
        for p in &c.points {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }
    }
    if let Some((fx, fz, fr)) = focus {
        // frame the requested circle only
        min_x = fx - fr;
        max_x = fx + fr;
        min_z = fz - fr;
        max_z = fz + fr;
    }
    if !min_x.is_finite() {
        (min_x, min_z, max_x, max_z) = (-50.0, -50.0, 50.0, 50.0);
    }
    let cx = ((min_x + max_x) / 2.0) as f32;
    let cz = ((min_z + max_z) / 2.0) as f32;
    let yaw: f32 = std::env::var("ROADKIT_YAW").ok().and_then(|v| v.parse().ok()).unwrap_or(0.55);
    let pitch: f32 = std::env::var("ROADKIT_PITCH").ok().and_then(|v| v.parse().ok()).unwrap_or(0.55);
    // rotate bbox corners into the camera frame: horizontal radius |x'| and
    // the depth span (perspective spread), vertical extent ~ depth * sin(pitch)
    let corners = [
        (min_x, min_z), (max_x, min_z), (min_x, max_z), (max_x, max_z),
    ];
    let (cy, sy) = (-(yaw as f64)).sin_cos(); // inverse yaw
    let mut rad_h = 10.0f64;
    let mut depth = 10.0f64;
    for (x, z) in corners {
        let dx = x - cx as f64;
        let dz = z - cz as f64;
        let x2 = dx * cy - dz * sy; // camera-right axis
        let z2 = dx * sy + dz * cy; // camera-forward axis
        rad_h = rad_h.max(x2.abs());
        depth = depth.max(z2.abs());
    }
    let vfov_half = 0.45f32; // OrbitCam::proj uses fov 0.9
    let tan_v = vfov_half.tan();
    let aspect = (w.max(1) as f32) / (h.max(1) as f32);
    let tan_h = tan_v * aspect;
    let dist_h = (rad_h as f32 / tan_h) * 1.12;
    let dist_v = (depth as f32 * pitch.sin() / tan_v) * 1.12;
    let dist = dist_h.max(dist_v).max(24.0);
    // perspective bias: the far half subtends fewer pixels than the near
    // half, so aim the target slightly toward the far side to visually
    // center the content
    let fwd = (-(yaw as f64).cos(), -(yaw as f64).sin());
    let bias = depth * 0.10;
    OrbitCam {
        zoom_vel: 0.0,
        target: Vec3::new((cx as f64 + fwd.0 * bias) as f32, 0.0, (cz as f64 + fwd.1 * bias) as f32),
        dist,
        yaw,
        pitch,
        ..Default::default()
    }
}

/// Frame the whole scene (default hero fit).
pub fn frame_cam(curves: &[roads::model::RoadCurve], w: usize, h: usize) -> OrbitCam {
    frame_cam_focused(curves, w, h, None)
}

/* ------------------------------------------------------------------ */
/* 2D plan renderer (the "draw in 2D" test view)                        */
/* ------------------------------------------------------------------ */

struct Canvas {
    px: Vec<u8>,
    w: usize,
    h: usize,
}

impl Canvas {
    fn new(w: usize, h: usize, bg: [u8; 3]) -> Self {
        let mut px = vec![0u8; w * h * 4];
        for i in 0..(w * h) {
            px[i * 4] = bg[0];
            px[i * 4 + 1] = bg[1];
            px[i * 4 + 2] = bg[2];
            px[i * 4 + 3] = 255;
        }
        Self { px, w, h }
    }

    fn set(&mut self, x: i64, y: i64, c: [u8; 3]) {
        if x < 0 || y < 0 || x >= self.w as i64 || y >= self.h as i64 {
            return;
        }
        let i = ((y as usize) * self.w + x as usize) * 4;
        self.px[i] = c[0];
        self.px[i + 1] = c[1];
        self.px[i + 2] = c[2];
        self.px[i + 3] = 255;
    }

    /// Filled circle in screen space.
    fn dot(&mut self, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
        let (x0, x1) = ((cx - r).floor() as i64, (cx + r).ceil() as i64);
        let (y0, y1) = ((cy - r).floor() as i64, (cy + r).ceil() as i64);
        for y in y0..=y1 {
            for x in x0..=x1 {
                let dx = x as f32 + 0.5 - cx;
                let dy = y as f32 + 0.5 - cy;
                if dx * dx + dy * dy <= r * r {
                    self.set(x, y, c);
                }
            }
        }
    }

    /// Thick line via perpendicular quad (screen space, meters already applied).
    fn thick(&mut self, a: (f32, f32), b: (f32, f32), half: f32, c: [u8; 3]) {
        let (ax, ay) = a;
        let (bx, by) = b;
        let dx = bx - ax;
        let dy = by - ay;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / len, dx / len);
        let pts = [
            (ax + nx * half, ay + ny * half),
            (bx + nx * half, by + ny * half),
            (bx - nx * half, by - ny * half),
            (ax - nx * half, ay - ny * half),
        ];
        self.poly(&pts, c);
    }

    /// Thick line with independent half-widths per end (taper trapezoid).
    fn thick2(&mut self, a: (f32, f32), b: (f32, f32), half_a: f32, half_b: f32, c: [u8; 3]) {
        let (ax, ay) = a;
        let (bx, by) = b;
        let dx = bx - ax;
        let dy = by - ay;
        let len = (dx * dx + dy * dy).sqrt().max(0.001);
        let (nx, ny) = (-dy / len, dx / len);
        let pts = [
            (ax + nx * half_a, ay + ny * half_a),
            (bx + nx * half_b, by + ny * half_b),
            (bx - nx * half_b, by - ny * half_b),
            (ax - nx * half_a, ay - ny * half_a),
        ];
        self.poly(&pts, c);
    }

    /// Convex polygon fill (screen space), scanline.
    fn poly(&mut self, pts: &[(f32, f32)], c: [u8; 3]) {
        if pts.len() < 3 {
            return;
        }
        let min_y = pts.iter().map(|p| p.1).fold(f32::MAX, f32::min).floor() as i64;
        let max_y = pts.iter().map(|p| p.1).fold(f32::MIN, f32::max).ceil() as i64;
        for y in min_y.max(0)..=max_y.min(self.h as i64 - 1) {
            let fy = y as f32 + 0.5;
            let mut xs: Vec<f32> = Vec::new();
            for i in 0..pts.len() {
                let a = pts[i];
                let b = pts[(i + 1) % pts.len()];
                if (a.1 <= fy && b.1 > fy) || (b.1 <= fy && a.1 > fy) {
                    let t = (fy - a.1) / (b.1 - a.1);
                    xs.push(a.0 + t * (b.0 - a.0));
                }
            }
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            for pair in xs.chunks(2) {
                if pair.len() == 2 {
                    let (x0, x1) = (pair[0].ceil() as i64, pair[1].floor() as i64);
                    for x in x0.max(0)..=x1.min(self.w as i64 - 1) {
                        self.set(x, y, c);
                    }
                }
            }
        }
    }

    fn take(self) -> Vec<u8> {
        self.px
    }
}

/// Top-down plan render: grass background, junction patches, per-curve
/// cross-section strips (pathway/shoulder/asphalt/median) + markings, props as dots.
pub fn plan_render(world: &World, zoom: f32, w: usize, h: usize) -> Vec<u8> {
    let mut cv = Canvas::new(w, h, [42, 48, 38]);

    // world → screen transform: fit curves bbox with margin, zoom multiplies scale
    let mut min = (f64::MAX, f64::MAX);
    let mut max = (f64::MIN, f64::MIN);
    for c in &world.scene.curves {
        for p in &c.points {
            min.0 = min.0.min(p.x);
            min.1 = min.1.min(p.z);
            max.0 = max.0.max(p.x);
            max.1 = max.1.max(p.z);
        }
    }
    if min.0 > max.0 {
        min = (-50.0, -50.0);
        max = (50.0, 50.0);
    }
    let pad = 24.0f32;
    let span_w = ((max.0 - min.0) as f32).max(20.0);
    let span_h = ((max.1 - min.1) as f32).max(20.0);
    let scale = (((w as f32 - 2.0 * pad) / span_w).min((h as f32 - 2.0 * pad) / span_h) * zoom).min(64.0);
    let cxw = ((min.0 + max.0) / 2.0) as f32;
    let czw = ((min.1 + max.1) / 2.0) as f32;
    let tx = |x: f32| -> f32 { (x - cxw) * scale + w as f32 / 2.0 };
    let ty = |z: f32| -> f32 { (z - czw) * scale + h as f32 / 2.0 };

    let asphalt = [96, 100, 108]; // brighter at overview scale
    let shoulder = [96, 94, 88];
    let concrete = [168, 164, 152];
    let median_c = [150, 146, 134];
    let grass = [62, 92, 52];
    let yellow = [235, 195, 40];
    let white = [232, 232, 226];

    // per-curve strips (per-sample widths — tapers render in 2D too)
    for curve in &world.scene.curves {
        let prof = curve.width_profile(4.0);
        if prof.len() < 2 {
            continue;
        }
        let p0 = &curve.points[0];
        let med_half = (p0.median / 2.0) as f32;
        let shoulder = (p0.shoulder_l.max(p0.shoulder_r)) as f32;
        let pathway = (p0.pathway_l.max(p0.pathway_r)) as f32;
        let pass = |cv: &mut Canvas, base: f64, c: [u8; 3]| {
            for i in 0..prof.len() - 1 {
                let (pa, ha) = prof[i];
                let (pb, hb) = prof[i + 1];
                let a = (tx(pa.x as f32), ty(pa.y as f32));
                let b = (tx(pb.x as f32), ty(pb.y as f32));
                // skip only zero-length samples (a 0.5 px gate turned
                // city-scale plans into dotted traces)
                if (a.0 - b.0).abs() + (a.1 - b.1).abs() < 0.02 {
                    continue;
                }
                // minimum stroke so sub-pixel road strips still read
                let min_px = if base == 0.0 { 2.2 } else { 1.2 };
                let ta = ((((ha + base) * scale as f64) as f32).max(min_px));
                let tb = ((((hb + base) * scale as f64) as f32).max(min_px));
                cv.thick2(a, b, ta, tb, c);
            }
        };
        let half_all = shoulder as f64 + if pathway > 0.0 { 2.0 } else { 0.0 };
        pass(&mut cv, half_all, if pathway > 0.0 { concrete } else { [96, 94, 88] });
        pass(&mut cv, shoulder as f64, [96, 94, 88]);
        pass(&mut cv, 0.0, asphalt);
        // absolute-width center strip (median, or the double-yellow)
        let strip_center = |cv: &mut Canvas, half: f32, c: [u8; 3]| {
            for i in 0..prof.len() - 1 {
                let (pa, _) = prof[i];
                let (pb, _) = prof[i + 1];
                let a = (tx(pa.x as f32), ty(pa.y as f32));
                let b = (tx(pb.x as f32), ty(pb.y as f32));
                if (a.0 - b.0).abs() + (a.1 - b.1).abs() < 0.5 {
                    continue;
                }
                cv.thick2(a, b, half * scale as f32, half * scale as f32, c);
            }
        };
        // painted line at a constant lateral offset (meters, clamped inside)
        let line_at = |cv: &mut Canvas, off: f64, half: f64, c: [u8; 3], dash: bool| {
            for (i, w) in prof.windows(2).enumerate() {
                if dash && i % 2 == 1 {
                    continue;
                }
                let (pa, ha) = w[0];
                let (pb, hb) = w[1];
                let dx = pb.x - pa.x;
                let dz = pb.y - pa.y;
                let len = (dx * dx + dz * dz).sqrt().max(0.001);
                let (nx, nz) = (-dz / len, dx / len);
                let oa = (off.min((ha - 0.2).max(0.0))) as f32;
                let ob = (off.min((hb - 0.2).max(0.0))) as f32;
                let p1 = (tx((pa.x + nx * oa as f64) as f32), ty((pa.y + nz * oa as f64) as f32));
                let p2 = (tx((pb.x + nx * ob as f64) as f32), ty((pb.y + nz * ob as f64) as f32));
                cv.thick(p1, p2, (half * scale as f64) as f32, c);
            }
        };
        if p0.median > 0.5 {
            strip_center(&mut cv, med_half, median_c);
            line_at(&mut cv, med_half as f64 + 0.15, 0.14, yellow, false);
            line_at(&mut cv, -(med_half as f64) - 0.15, 0.14, yellow, false);
        } else {
            strip_center(&mut cv, 0.28, yellow);
        }
        // edge lines: per-sample offset = half(i) - 0.35 (both signs)
        for sign in [-1.0f64, 1.0] {
            for (i, w) in prof.windows(2).enumerate() {
                let (pa, ha) = w[0];
                let (pb, hb) = w[1];
                let dx = pb.x - pa.x;
                let dz = pb.y - pa.y;
                let len = (dx * dx + dz * dz).sqrt().max(0.001);
                let (nx, nz) = (-dz / len, dx / len);
                let oa = ((ha - 0.35) * sign) as f32;
                let ob = ((hb - 0.35) * sign) as f32;
                let p1 = (tx((pa.x + nx * oa as f64) as f32), ty((pa.y + nz * oa as f64) as f32));
                let p2 = (tx((pb.x + nx * ob as f64) as f32), ty((pb.y + nz * ob as f64) as f32));
                cv.thick(p1, p2, (0.14 * scale as f64) as f32, white);
            }
        }
        // dashed lane separators per side (per-sample lateral offsets)
        let lanes = p0.lanes.max(1);
        let left_n = if p0.oneway { 0 } else { (lanes / 2).max(1) };
        let right_n = if p0.oneway { lanes } else { lanes - left_n };
        for j in 1..left_n {
            let base = (p0.median / 2.0 + j as f64 * p0.lane_width) * -1.0;
            line_at(&mut cv, base, 0.1, white, true);
        }
        for j in 1..right_n {
            let base = p0.median / 2.0 + j as f64 * p0.lane_width;
            line_at(&mut cv, base, 0.1, white, true);
        }
    }

    // junction patches on top (the GRG junction mesh covers the road ends)
    for j in &world.scene.junctions {
        let pts: Vec<(f32, f32)> = j.outline.iter().map(|(x, z)| (tx(*x as f32), ty(*z as f32))).collect();
        cv.poly(&pts, asphalt);
        if let Some((ix, iz, r)) = j.island {
            let steps = 24;
            let ring: Vec<(f32, f32)> =
                (0..steps).map(|k| { let a = k as f64 / steps as f64 * std::f64::consts::TAU; (tx((ix + a.cos() * r) as f32), ty((iz + a.sin() * r) as f32)) }).collect();
            cv.poly(&ring, grass);
        }
    }

    // props as dots
    for p in &world.scene.props {
        let c = match p.asset.as_str() {
            "vehicle" => [220, 80, 60],
            "tree" => [80, 140, 70],
            "streetlight" => [230, 210, 120],
            "milestone" => [200, 200, 210],
            "junctionMarker" | "junctionMarkerA" => [90, 200, 230],
            _ => [180, 180, 190],
        };
        cv.dot(tx(p.pos.x), ty(p.pos.z), 3.5, c);
    }

    // junction labels
    let _ = &grass;
    cv.take()
}

/// Named entry: resolve the script, build the world, render each view.
pub fn render_named(script: &str, views: &[(String, std::path::PathBuf)], zoom: f32, w: u32, h: u32) -> Result<(), String> {
    render_named_ss(script, views, zoom, w, h, 1)
}

/// Same with a supersample factor (xtask screenshots use ss=2).
pub fn render_named_ss(script: &str, views: &[(String, std::path::PathBuf)], zoom: f32, w: u32, h: u32, ss: usize) -> Result<(), String> {
    render_named_focus(script, views, zoom, w, h, ss, None)
}

/// Same with a supersample factor + optional focus area (x, z, radius_m).
pub fn render_named_focus(
    script: &str,
    views: &[(String, std::path::PathBuf)],
    zoom: f32,
    w: u32,
    h: u32,
    ss: usize,
    focus: Option<(f64, f64, f64)>,
) -> Result<(), String> {
    render_named_sim(script, views, zoom, w, h, ss, focus, 0.0, 14.0)
}

/// Full-control entry: supersample + focus + deterministic traffic snapshot.
#[allow(clippy::too_many_arguments)]
pub fn render_named_sim(
    script: &str,
    views: &[(String, std::path::PathBuf)],
    zoom: f32,
    w: u32,
    h: u32,
    ss: usize,
    focus: Option<(f64, f64, f64)>,
    sim_seconds: f64,
    density: f64,
) -> Result<(), String> {
    let (world, _path) = build_world_named(script)?;
    for (view, out) in views {
        let rgba = render_view_sim(&world, view, zoom, w as usize, h as usize, ss, focus, sim_seconds, density)?;
        encode_png(out, &rgba, w, h)?;
        println!("wrote {} ({} x {})", out.display(), w, h);
    }
    for line in world.log_lines() {
        println!("  | {line}");
    }
    Ok(())
}

/// Render the 3D view with an EXPLICIT camera (MCP render_3d tool).
/// `focus` = Some((x, z, radius_m)) frames that world area instead of using
/// the camera directly.
#[allow(clippy::too_many_arguments)]
pub fn render_3d_camera(
    world: &World,
    cam: &OrbitCam,
    w: usize,
    h: usize,
    ss: usize,
    sim_seconds: f64,
    density: f64,
) -> Vec<u8> {
    let mut props = world.scene.props.clone();
    if sim_seconds > 0.0 {
        let mut sim = roadkit::TrafficSim::new(
            &world.scene.curves,
            &world.scene.junctions,
            roadkit::SimSettings { density, ..Default::default() },
        );
        let steps = (sim_seconds / 0.1).ceil() as usize;
        for _ in 0..steps {
            sim.step(0.1, &world.scene.curves);
        }
        props.extend(sim.prop_instances(&world.scene.curves));
    }
    let scene = build_scene_with_bg(&world.scene.curves, &world.scene.junctions, &props, &world.library, true, roadkit::ENV_DAY.clear);
    let env = roadkit::scene::Environment::from_env();
    if ss <= 1 {
        return roadkit::scene::render_env_shaded(&scene, &world.library, &world.textures, cam, w, h, &env, &[], true, roadkit::raster::ShadeMode::Solid);
    }
    let big = roadkit::scene::render_env_shaded(&scene, &world.library, &world.textures, cam, w * ss, h * ss, &env, &[], true, roadkit::raster::ShadeMode::Solid);
    box_downsample(&big, w * ss, h * ss, ss)
}

/// Top-down plan view of the world (2D blueprint render).
pub fn plan_rgba(world: &World, zoom: f32, w: usize, h: usize) -> Vec<u8> {
    plan_render(world, zoom, w, h)
}

pub fn encode_png(out: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
    let mut buf = Vec::new();
    {
        use image::ImageBuffer;
        let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_raw(w, h, rgba.to_vec()).ok_or("png buffer size mismatch")?;
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .map_err(|e| format!("png encode: {e}"))?;
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &buf).map_err(|e| format!("write {}: {e}", out.display()))
}

/* ---- Dar es Salaam (JICA + OSM) scenario renders ------------------ */

/// Build the DSM world: real OSM ground network + the JICA TR2 overlay
/// (Kijazi split flyover, Tazara 540 m flyover, BRT median, signals).
pub fn build_world_dsm() -> Result<(World, roadkit::SignalRegistry), String> {
    let dsm = roadkit::build_dsm();
    let mut st = SceneState::default();
    let n = dsm.curves.len();
    let n_sig = dsm.registry.entries.len();
    st.curves = dsm.curves;
    st.console.push(format!("dsm: {n} curves · {n_sig} controlled junctions (JICA TR2)"));
    st.auto_join();
    let assets = {
        let mut c = std::env::current_dir().map_err(|e| e.to_string())?;
        loop {
            if c.join("assets/glb").exists() {
                break c.join("assets");
            }
            if !c.pop() {
                return Err("assets dir not found".into());
            }
        }
    };
    Ok((
        World {
            library: AssetLibrary::load_dir(&assets),
            textures: TextureLibrary::load_dir(&assets.join("textures")),
            scene: st,
        },
        dsm.registry,
    ))
}

/// Render the DSM scenario: one plan view + per-site 3D shots with the
/// registry-driven signals + left-hand traffic. Each site is rendered in
/// its own LOCAL frame (curves shifted so the site sits at the origin) —
/// the 9 km corridor would otherwise melt into the distance fog.
#[allow(clippy::too_many_arguments)]
pub fn render_dsm(
    sites: &[(f64, f64, &str)],
    w: u32,
    h: u32,
    ss: usize,
    sim_seconds: f64,
    density: f64,
) -> Result<Vec<String>, String> {
    let mut outs = Vec::new();
    // whole-corridor plan view (raw coordinates) — multi-site runs only
    // (single-site --dsm-site renders skip it)
    if sites.len() > 1 {
        let dsm_full = roadkit::build_dsm();
        let mut st = SceneState::default();
        st.curves = dsm_full.curves.clone();
        st.auto_join();
        let assets = find_assets_dir_cwd()?;
        let world_plan = World {
            library: AssetLibrary::load_dir(&assets),
            textures: TextureLibrary::load_dir(&assets.join("textures")),
            scene: st,
        };
        let plan = plan_render(&world_plan, 1.0, w as usize, h as usize);
        let plan_out = std::path::PathBuf::from("dsm_plan.png");
        encode_png(&plan_out, &plan, w, h)?;
        outs.push(plan_out.display().to_string());
    }

    let assets = find_assets_dir_cwd()?;
    let library = AssetLibrary::load_dir(&assets);
    let textures = TextureLibrary::load_dir(&assets.join("textures"));
    for (sx, sz, name) in sites {
        // LOCAL scenario: curves within ~700 m, site at the origin — the
        // 9 km corridor otherwise melts into the distance fog
        let dsm = roadkit::build_dsm_local((*sx, *sz), 700.0);
        let junctions = roads::junction::detect(&dsm.curves, &Default::default());
        let focus_r: f64 = std::env::var("ROADKIT_FOCUS_R").ok().and_then(|v| v.parse().ok()).unwrap_or(250.0);
        let mut cam = frame_cam_focused(&dsm.curves, w as usize, h as usize, Some((0.0, 0.0, focus_r)));
        {
            // build_scene recenters geometry by the bbox centre — aim the
            // camera at the SAME shifted focus point
            let (cx, cz, _, _) = roadkit::scene_recenter(&dsm.curves);
            cam.target = gdmath::vec3::Vec3::new(-cx, 0.0, -cz);
        }
        let mut props: Vec<roadkit::PropInstance> = roadkit::dsm::dsm_props_capped(&dsm.curves, 720.0);
        if sim_seconds > 0.0 {
            let mut sim = roadkit::TrafficSim::new(
                &dsm.curves,
                &junctions,
                roadkit::SimSettings { density, left_hand: true, ..Default::default() },
            )
            .with_registry(&dsm.registry, &junctions, &dsm.curves);
            let steps = (sim_seconds / 0.1).ceil() as usize;
            for _ in 0..steps {
                sim.step(0.1, &dsm.curves);
            }
            props.extend(sim.prop_instances(&dsm.curves));
        }
        let enrich: Vec<roads::mesh3d::MeshPart> = if std::env::var("ROADKIT_NOEXTRAS").is_ok() { Vec::new() } else { roadkit::dsm::dsm_buildings(&dsm.curves) };
        let scene = roadkit::scene::build_scene_enriched(
            &dsm.curves,
            &junctions,
            &props,
            &[],
            &library,
            true,
            false,
            None,
            roadkit::ENV_DAY.clear_rgb(),
            &enrich,
        );
        eprintln!("[dsm] {name}: cam.dist={:.0} radius={:.0} curves={} junctions={} lights={}", cam.dist, scene.radius_m, dsm.curves.len(), junctions.len(), dsm.registry.entries.len());
        let shade = shade_from_env();
        let env = roadkit::scene::Environment::from_env();
        let (rw, rh) = if shade == roadkit::raster::ShadeMode::Quality {
            (w as usize, h as usize) // Quality does its own 2x SSAA
        } else if ss > 1 {
            (w as usize * ss, h as usize * ss)
        } else {
            (w as usize, h as usize)
        };
        let big = roadkit::scene::render_env_shaded(&scene, &library, &textures, &cam, rw, rh, &env, &[], true, shade);
        let rgba = if rw != w as usize {
            box_downsample(&big, rw, rh, rw / w as usize)
        } else {
            big
        };
        let out = std::path::PathBuf::from(format!("dsm_{name}.png"));
        encode_png(&out, &rgba, w, h)?;
        outs.push(out.display().to_string());
    }
    Ok(outs)
}

/// Debug: replicate the APP's exact path (full scene, center-subtracted,
/// camera at the negated bbox center) to diagnose the thin-roads bug.
pub fn render_dsm_appcam(w: u32, h: u32) -> Result<(), String> {
    let dsm = roadkit::build_dsm();
    let junctions = roads::junction::detect(&dsm.curves, &Default::default());
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for c in &dsm.curves {
        for p in &c.points {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }
    }
    let cx = ((min_x + max_x) / 2.0) as f32;
    let cz = ((min_z + max_z) / 2.0) as f32;
    let assets = find_assets_dir_cwd()?;
    let library = AssetLibrary::load_dir(&assets);
    let textures = TextureLibrary::load_dir(&assets.join("textures"));
    let props = roadkit::dsm::dsm_props_capped(&dsm.curves, 720.0);
    let enrich = roadkit::dsm::dsm_buildings(&dsm.curves);
    let scene = roadkit::scene::build_scene_enriched(
        &dsm.curves,
        &junctions,
        &props,
        &[],
        &library,
        true,
        false,
        None,
        roadkit::ENV_DAY.clear_rgb(),
        &enrich,
    );
    eprintln!("[appcam] scene.radius={} center=({cx},{cz})", scene.radius_m);
    let cam = OrbitCam {
        zoom_vel: 0.0,
        target: gdmath::Vec3::new(-cx, 0.0, -cz),
        dist: 420.0,
        pitch: 0.62,
        yaw: 0.65,
        ..Default::default()
    };
    let env = roadkit::scene::Environment::from_env();
    let rgba = roadkit::scene::render_env_shaded(&scene, &library, &textures, &cam, w as usize, h as usize, &env, &[], true, roadkit::raster::ShadeMode::Solid);
    encode_png(std::path::Path::new("dsm_appcam.png"), &rgba, w, h)?;
    Ok(())
}

fn find_assets_dir_cwd() -> Result<std::path::PathBuf, String> {
    let mut c = std::env::current_dir().map_err(|e| e.to_string())?;
    loop {
        if c.join("assets/glb").exists() {
            return Ok(c.join("assets"));
        }
        if !c.pop() {
            return Err("assets dir not found".into());
        }
    }
}
