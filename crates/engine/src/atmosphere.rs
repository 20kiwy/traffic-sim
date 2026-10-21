//! Atmosphere / sky / fog / tonemap math — a CPU port of the Egregoria
//! engine's WGSL shaders (https://github.com/uriopass/egregoria, MIT), files:
//!
//! - `assets/shaders/atmosphere.wgsl` — the single-scattering atmosphere
//!   (analytic ray-sphere `rsi`, Rayleigh/Mie phase functions, 12 primary x
//!   4 secondary ray-march steps, exponential per-step attenuation), the
//!   painted background gradient added to infinite-depth (sky) rays, and the
//!   `totalRlh *= 0.4; totalMie *= 0.8` aerial-perspective damping applied to
//!   finite-depth (fog) rays;
//! - `assets/shaders/background.wgsl` — the additive sun disc
//!   `max(pos.z,0) * 10000 * smoothstep(0.99993, 1.0, dot(fsun, pos))`;
//! - `assets/shaders/tonemap.wgsl` — `1.830796 * c / (c * 1.24068 + 1.682186)`;
//! - `assets/shaders/pbr/render.wgsl` — the autoexposure term
//!   `1 + smoothstep(0, 0.1, -sun.z) * 10` (applied before tonemap) and the
//!   post-tonemap dither;
//! - `assets/shaders/dither.wgsl` — `(blue_noise - 0.5) / 255` (see
//!   [`dither`] for the CPU substitution of the noise texture);
//! - `assets/shaders/fog.wgsl` — the aerial perspective pass
//!   `atmosphere(-V, sun, dist * 0.2)`, cached here as [`FogLut`].
//!
//! # Coordinate frames
//! The WGSL is Z-up: up = `+z`, ray origin `(0, 0, 6371500)`. This crate is
//! Y-up (roads convention: x east, y up, z south — see [`crate::sun`]). The
//! raw Z-up core is private; the public functions take/return Y-up
//! directions and swap the y/z components internally — the exact same swap
//! [`crate::sun::sun_dir`] performs, so its output feeds these functions
//! directly. (The y/z swap is a reflection; the radiance math only uses dot
//! products and heights above the planet, both invariant under it.)
//!
//! All returned colors are LINEAR HDR radiance. Display pipeline, matching
//! the shaders: `sky = atmosphere(...)`; add fog ADDITIVELY (`ambient + Lo +
//! fog` in `pbr/render.wgsl`, `final_rgb += fog` in `water.frag.wgsl`);
//! scale by [`autoexposure`]; apply [`tonemap`]; add [`dither`].
//!
//! # Documented deviations from the WGSL
//! - [`background_light`] clamps the gradient's `sqrt(r.z)` argument at 0:
//!   the shader computes `sqrt` of a negative for below-horizon rays (NaN)
//!   and relies on those pixels being occluded by terrain; for `r.z >= 0`
//!   (everything visible) this is identical to the shader.
//! - Directions are normalized at the API boundary — the WGSL documents
//!   "r and pSun are normalized" (the GPU receives a normalized sun from
//!   `RenderParams`; CPU callers may pass the un-normalized
//!   [`crate::sun::sun_dir`] output).
//! - [`dither`] substitutes a deterministic hash for the blue-noise texture.
//! - [`SkyLut`] stores `atmosphere_yup(dir, sun, INFINITY)` alone: the WGSL
//!   `atmosphere()` already adds the painted background inside its
//!   `maxDepth > 1e30` branch, so adding [`background_light`] again would
//!   double it.

use gdmath::Vec3;

const PI: f32 = std::f32::consts::PI;
const TAU: f32 = std::f32::consts::TAU;
const FRAC_PI_2: f32 = std::f32::consts::FRAC_PI_2;

/// The WGSL constants of `atmosphere.wgsl`, verbatim.
pub struct AtmosphereParams;

impl AtmosphereParams {
    /// Primary ray-march steps (`iSteps`).
    pub const I_STEPS: i32 = 12;
    /// Secondary ray-march steps (`jSteps`).
    pub const J_STEPS: i32 = 4;
    /// Intensity of the sun (`iSun`).
    pub const I_SUN: f32 = 22.0;
    /// Radius of the planet in meters (`rPlanet`).
    pub const R_PLANET: f32 = 6371e3;
    /// Radius of the atmosphere in meters (`rAtmos`).
    pub const R_ATMOS: f32 = 6471e3;
    /// Rayleigh scattering coefficient (`kRlh`).
    pub const K_RLH: [f32; 3] = [5.5e-6, 13.0e-6, 22.4e-6];
    /// Mie scattering coefficient (`kMie`).
    pub const K_MIE: f32 = 21e-6;
    /// Rayleigh scale height (`shRlh`).
    pub const SH_RLH: f32 = 8e3;
    /// Mie scale height (`shMie`).
    pub const SH_MIE: f32 = 1.2e3;
    /// Mie preferred scattering direction (`g`).
    pub const G: f32 = 0.758;
    /// Ray origin height (`r0 = vec3(0, 0, 6371500)` — 500 m above the planet).
    pub const RAY_ORIGIN_Z: f32 = 6371500.0;
    /// The `maxDepth` sentinel the shaders use for sky rays (`3.40282347E+38`
    /// in `background.wgsl`, `1e38` in `water.frag.wgsl`); anything above
    /// `1e30` selects the sky branch (painted background, no damping).
    pub const SKY_DEPTH: f32 = 3.40282347e38;
}

// ---------------------------------------------------------------------------
// small vec3 helpers (kept local — gdmath::Vec3 is intentionally minimal)
// ---------------------------------------------------------------------------

/// Y-up <-> Z-up frame swap (an involution), matching `sun::sun_dir`.
#[inline]
fn swap_yz(v: Vec3) -> Vec3 {
    Vec3::new(v.x, v.z, v.y)
}

#[inline]
fn splat3(s: f32) -> Vec3 {
    Vec3::new(s, s, s)
}

/// component-wise multiply (missing from gdmath::Vec3)
#[inline]
fn mul3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(a.x * b.x, a.y * b.y, a.z * b.z)
}

#[inline]
fn neg3(v: Vec3) -> Vec3 {
    Vec3::new(-v.x, -v.y, -v.z)
}

#[inline]
fn exp3(v: Vec3) -> Vec3 {
    Vec3::new(v.x.exp(), v.y.exp(), v.z.exp())
}

#[inline]
fn saturate(x: f32) -> f32 {
    x.clamp(0.0, 1.0)
}

/// GLSL/WGSL `smoothstep(edge0, edge1, x)`.
#[inline]
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[inline]
fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

// ---------------------------------------------------------------------------
// atmosphere.wgsl core (Z-up, private)
// ---------------------------------------------------------------------------

/// Ray-sphere intersection for a sphere centered at the origin — exact port
/// of `rsi` from `atmosphere.wgsl`. No intersection when `t.0 > t.1`
/// (the shader's sentinel `(1e5, -1e5)` is returned, and the caller's
/// `p.y = rsi(...).x` logic depends on that exact sentinel).
fn rsi(r0: Vec3, rd: Vec3, sr: f32) -> (f32, f32) {
    let a = rd.dot(rd);
    let b = 2.0 * rd.dot(r0);
    let c = r0.dot(r0) - sr * sr;
    let d = b * b - 4.0 * a * c;
    if d < 0.0 {
        return (1e5, -1e5);
    }
    ((-b - d.sqrt()) / (2.0 * a), (-b + d.sqrt()) / (2.0 * a))
}

#[inline]
fn ray_origin() -> Vec3 {
    Vec3::new(0.0, 0.0, AtmosphereParams::RAY_ORIGIN_Z)
}

/// The painted background gradient of `atmosphere.wgsl` (lines ~122-129),
/// Z-up:
/// `mix(vec3(0.0116,0.027,0.0423), vec3(0.0036,0.013,0.0194),
///   saturate(2.0*sqrt(r.z))) * 0.4 * smoothstep(-0.3, 0.1, r.z)`
///
/// Deviation: the shader's `sqrt(r.z)` is NaN for `r.z < 0` (the result is
/// multiplied by a 0 `smoothstep` there and those pixels are occluded by
/// terrain); we clamp at 0 so CPU results stay finite. Identical for
/// `r.z >= 0`.
fn background_light_zup(r: Vec3) -> Vec3 {
    let gradient = saturate(2.0 * r.z.max(0.0).sqrt());
    Vec3::new(0.0116, 0.027, 0.0423) // light blue (horizon)
        .lerp(Vec3::new(0.0036, 0.013, 0.0194), gradient) // dark blue
        * 0.4 // power
        * smoothstep(-0.3, 0.1, r.z) // black at bottom
}

/// Exact port of `atmosphere(r, pSun, maxDepth)` from `atmosphere.wgsl`.
/// `r` and `p_sun` must be unit vectors in the shader's Z-up frame.
fn atmosphere_zup(r: Vec3, p_sun: Vec3, max_depth: f32) -> Vec3 {
    let r0 = ray_origin();
    let k_rlh = splat3_from(AtmosphereParams::K_RLH);

    // Calculate the step size of the primary ray.
    let mut p = rsi(r0, r, AtmosphereParams::R_ATMOS);
    if p.0 > p.1 {
        return Vec3::ZERO;
    }
    p.1 = rsi(r0, r, AtmosphereParams::R_PLANET).0;

    let mut i_dist = max_depth.min(p.1 - p.0);

    if r.z < 0.0 {
        i_dist = (1000.0 / -r.z).min(i_dist);
    }

    let i_step_size = i_dist / AtmosphereParams::I_STEPS as f32;

    // Initialize the primary ray time.
    let mut i_time = i_step_size * 0.375;

    // Accumulators for Rayleigh and Mie scattering.
    let mut total_rlh = Vec3::ZERO;
    let mut total_mie = Vec3::ZERO;

    // Optical depth accumulators for the primary ray.
    let mut i_od_rlh = 0.0f32;
    let mut i_od_mie = 0.0f32;

    // Rayleigh and Mie phases.
    let mu = r.dot(p_sun);
    let mumu = mu * mu;
    let gg = AtmosphereParams::G * AtmosphereParams::G;
    let p_rlh = 3.0 / (16.0 * PI) * (1.0 + mumu);
    let p_mie = 3.0 / (8.0 * PI) * ((1.0 - gg) * (mumu + 1.0))
        / ((1.0 + gg - 2.0 * mu * AtmosphereParams::G).powf(1.5) * (2.0 + gg));

    // Sample the primary ray.
    for _ in 0..AtmosphereParams::I_STEPS {
        // Primary ray sample position and height.
        let i_pos = r0 + r * i_time;
        let i_height = i_pos.len() - AtmosphereParams::R_PLANET;

        // Optical depth of the Rayleigh and Mie scattering for this step.
        let od_step_rlh = (-i_height / AtmosphereParams::SH_RLH).exp() * i_step_size;
        let od_step_mie = (-i_height / AtmosphereParams::SH_MIE).exp() * i_step_size;

        i_od_rlh += od_step_rlh;
        i_od_mie += od_step_mie;

        // Secondary ray step size and optical depth.
        let j_step_size =
            rsi(i_pos, p_sun, AtmosphereParams::R_ATMOS).1 / AtmosphereParams::J_STEPS as f32;
        let mut j_time = 0.0f32;
        let mut j_od_rlh = 0.0f32;
        let mut j_od_mie = 0.0f32;
        for _ in 0..AtmosphereParams::J_STEPS {
            let j_pos = i_pos + p_sun * (j_time + j_step_size * 0.5);
            let j_height = j_pos.len() - AtmosphereParams::R_PLANET;
            j_od_rlh += (-j_height / AtmosphereParams::SH_RLH).exp() * j_step_size;
            j_od_mie += (-j_height / AtmosphereParams::SH_MIE).exp() * j_step_size;
            j_time += j_step_size;
        }

        // Attenuation.
        let attn = exp3(neg3(
            splat3(AtmosphereParams::K_MIE * (i_od_mie + j_od_mie))
                + mul3(k_rlh, splat3(i_od_rlh + j_od_rlh)),
        ));

        // Accumulate scattering.
        total_rlh = total_rlh + attn * od_step_rlh;
        total_mie = total_mie + attn * od_step_mie;

        i_time += i_step_size;
    }

    let background = if max_depth > 1e30 {
        background_light_zup(r)
    } else {
        // Aerial-perspective damping, verbatim from the shader's else-branch
        // (`fog.wgsl` / `water.frag.wgsl` call with finite `dist * 0.2`).
        total_rlh = total_rlh * 0.4;
        total_mie = total_mie * 0.8;
        Vec3::ZERO
    };

    // Final color: backgroundLight + iSun * (pRlh*kRlh*totalRlh + pMie*kMie*totalMie)
    background
        + (mul3(k_rlh, total_rlh) * p_rlh
            + mul3(splat3(AtmosphereParams::K_MIE), total_mie) * p_mie)
            * AtmosphereParams::I_SUN
}

#[inline]
fn splat3_from(c: [f32; 3]) -> Vec3 {
    Vec3::new(c[0], c[1], c[2])
}

// ---------------------------------------------------------------------------
// public Y-up API
// ---------------------------------------------------------------------------

/// The atmosphere radiance for a view ray, linear HDR `[r, g, b]`.
///
/// Port of `atmosphere()` from `assets/shaders/atmosphere.wgsl` (egregoria,
/// MIT); directions converted from our Y-up frame to the shader's Z-up frame
/// internally (y/z swap, see the module docs).
///
/// * `dir_yup` — normalized view direction (normalized defensively here;
///   the WGSL contract is "r and pSun are normalized").
/// * `sun_yup` — direction TOWARD the sun (e.g. [`crate::sun::sun_dir`]).
/// * `max_depth` — ray length in meters:
///   - `> 1e30` (sky rays — use [`AtmosphereParams::SKY_DEPTH`]): adds the
///     painted background gradient, no damping;
///   - finite (aerial perspective — the shader calls with `dist * 0.2`):
///     no background, `totalRlh *= 0.4; totalMie *= 0.8` damping, exactly
///     as the WGSL.
pub fn atmosphere_yup(dir_yup: Vec3, sun_yup: Vec3, max_depth: f32) -> [f32; 3] {
    let r = swap_yz(dir_yup);
    let s = swap_yz(sun_yup);
    if r.len() < 1e-9 || s.len() < 1e-9 {
        return [0.0; 3];
    }
    let v = atmosphere_zup(r.normalized(), s.normalized(), max_depth);
    [v.x, v.y, v.z]
}

/// The painted background gradient added to sky rays (`atmosphere.wgsl`
/// lines ~122-129), Y-up. Only meaningful near/above the horizon
/// (`smoothstep(-0.3, 0.1, up)`); see the NaN-guard note on
/// [`background_light_zup`].
///
/// Note: [`atmosphere_yup`] with an infinite `max_depth` ALREADY includes
/// this term (it lives inside the WGSL `atmosphere()`), so do not add it a
/// second time — [`SkyLut`] is the composed result.
pub fn background_light(dir_yup: Vec3) -> [f32; 3] {
    let v = background_light_zup(swap_yz(dir_yup.normalized()));
    [v.x, v.y, v.z]
}

/// The additive sun-disc radiance from `background.wgsl` line 48:
/// `max(pos.z, 0) * 10000 * smoothstep(0.99993, 1.0, dot(fsun, pos))`,
/// in our Y-up frame (up = `+y` instead of the shader's `+z`).
///
/// Returns the scalar radiance to ADD to the sky color (the disc is pure
/// white in linear space). `sun_yup` / `dir_yup` are normalized defensively.
pub fn sun_disc(sun_yup: Vec3, dir_yup: Vec3) -> f32 {
    let pos = dir_yup.normalized();
    let fsun = sun_yup.normalized();
    pos.y.max(0.0) * 10000.0 * smoothstep(0.99993, 1.0, fsun.dot(pos))
}

/// Tonemap from `tonemap.wgsl` (egregoria, MIT):
/// `1.830796 * c / (c * 1.24068 + 1.682186)` per channel — a fitted
/// approximation of `1 - exp(-c)`. Plateau = `1.830796 / 1.24068 ≈ 1.4757`.
pub fn tonemap(c: [f32; 3]) -> [f32; 3] {
    [
        1.830796 * c[0] / (c[0] * 1.24068 + 1.682186),
        1.830796 * c[1] / (c[1] * 1.24068 + 1.682186),
        1.830796 * c[2] / (c[2] * 1.24068 + 1.682186),
    ]
}

/// Autoexposure from `pbr/render.wgsl`:
/// `1.0 + smoothstep(0.0, 0.1, -sun.z) * 10.0` where `sun.z` is the Z-up
/// height of the sun — here `sun_y` is our Y-up sun height
/// (e.g. `sun_dir_yup(tod).y`). Day: 1.0; deep night: 11.0.
pub fn autoexposure(sun_y: f32) -> f32 {
    1.0 + smoothstep(0.0, 0.1, -sun_y) * 10.0
}

/// Per-pixel dither, port of `dither.wgsl`'s `(noise - 0.5) / 255`.
///
/// SUBSTITUTION (documented): the WGSL samples a blue-noise texture
/// `t_bnoise` at `frag / 512.0`; we have no texture on CPU, so a small
/// deterministic hash — the classic `fract(sin(dot(p, vec2(12.9898, 78.233)))
/// * 43758.5453)` — stands in, computed in f64 for stable cross-platform
/// results. Output stays in the shader's `(-0.5, 0.5) / 255` range and is
/// meant to be added AFTER tonemap (`pbr/render.wgsl`: `color += dither`).
pub fn dither(px: u32, py: u32) -> f32 {
    let h = (f64::from(px) * 12.9898 + f64::from(py) * 78.233).sin() * 43758.5453;
    let h = h - h.floor(); // fract, as GLSL
    ((h - 0.5) / 255.0) as f32
}

// ---------------------------------------------------------------------------
// SkyLut — lat-long cache of the full sky
// ---------------------------------------------------------------------------

/// Lat-long cache of the complete sky (atmosphere + painted background) in
/// LINEAR HDR, 256x128, built per sun change and consumed per-pixel by the
/// software rasterizer (the CPU stand-in for `background.wgsl` evaluating
/// `atmosphere()` per fragment).
///
/// Each cell stores `atmosphere_yup(dir, sun, INFINITY)` — the shader's sky
/// path, which already includes the painted background gradient (see the
/// module docs for why [`background_light`] is NOT added on top).
#[derive(Clone, Debug)]
pub struct SkyLut {
    /// Normalized Y-up sun direction the LUT was built with (sanity check).
    pub sun: Vec3,
    /// Row-major `data[j * WIDTH + i]`; `i` = longitude cell, `j` = latitude
    /// row (0 = south pole row).
    pub data: Vec<[f32; 3]>,
}

impl SkyLut {
    pub const WIDTH: usize = 256;
    pub const HEIGHT: usize = 128;

    /// Direction of cell center `(i, j)` in the Y-up frame — the inverse of
    /// [`SkyLut::sample`]'s lat-long mapping.
    fn cell_dir(i: usize, j: usize) -> Vec3 {
        let theta = TAU * (i as f32 + 0.5) / Self::WIDTH as f32 - PI;
        let phi = PI * (j as f32 + 0.5) / Self::HEIGHT as f32 - FRAC_PI_2;
        let cp = phi.cos();
        Vec3::new(cp * theta.cos(), phi.sin(), cp * theta.sin())
    }

    /// Builds the LUT for a sun direction (normalized defensively). 32768
    /// atmosphere evaluations of 12x4 steps — tens of milliseconds.
    pub fn build(sun_yup: Vec3) -> Self {
        let sun = sun_yup.normalized();
        let mut data = vec![[0.0f32; 3]; Self::WIDTH * Self::HEIGHT];
        for j in 0..Self::HEIGHT {
            for i in 0..Self::WIDTH {
                let dir = Self::cell_dir(i, j);
                data[j * Self::WIDTH + i] = atmosphere_yup(dir, sun, f32::INFINITY);
            }
        }
        SkyLut { sun, data }
    }

    /// Bilinear lat-long sample at a view direction (Y-up, normalized
    /// defensively). Longitude wraps; latitude clamps at the poles.
    pub fn sample(&self, dir_yup: Vec3) -> [f32; 3] {
        let d = dir_yup.normalized();
        let theta = d.z.atan2(d.x);
        let phi = d.y.clamp(-1.0, 1.0).asin();
        // cell-space coordinates: u in [0, WIDTH) (wraps), v in [0, HEIGHT-1]
        let u = ((theta + PI) / TAU * Self::WIDTH as f32 - 0.5).rem_euclid(Self::WIDTH as f32);
        let v = ((phi + FRAC_PI_2) / PI * Self::HEIGHT as f32 - 0.5)
            .clamp(0.0, (Self::HEIGHT - 1) as f32);
        let i0 = (u.floor() as usize).min(Self::WIDTH - 1);
        let fu = u - i0 as f32;
        let i1 = (i0 + 1) % Self::WIDTH;
        let j0 = v.floor() as usize;
        let fv = v - j0 as f32;
        let j1 = (j0 + 1).min(Self::HEIGHT - 1);
        let a = self.data[j0 * Self::WIDTH + i0];
        let b = self.data[j0 * Self::WIDTH + i1];
        let c = self.data[j1 * Self::WIDTH + i0];
        let d2 = self.data[j1 * Self::WIDTH + i1];
        let top = lerp3(a, b, fu);
        let bottom = lerp3(c, d2, fu);
        lerp3(top, bottom, fv)
    }
}

// ---------------------------------------------------------------------------
// FogLut — aerial perspective cache
// ---------------------------------------------------------------------------

/// Aerial-perspective (fog) cache: 5x5 view directions (yaw/pitch offsets of
/// -30/-15/0/15/30 degrees around a view direction, stored as unit Y-up
/// dirs) x 24 log-spaced distances (50 m .. 20000 m), values = LINEAR HDR
/// `atmosphere_yup(dir, sun, dist * 0.2)` — the exact call `fog.wgsl` makes
/// (`atmosphere(-V, sun, dist * 0.2)`), so the shader's aerial damping
/// (`totalRlh *= 0.4; totalMie *= 0.8`, applied by the WGSL inside its
/// finite-`maxDepth` branch) is reproduced by construction.
///
/// IMPORTANT: fog is ADDITIVE. The shaders compose the final color as
/// `ambient + Lo + fog` (`pbr/render.wgsl`) / `final_rgb += fog`
/// (`water.frag.wgsl`) in LINEAR space BEFORE tonemapping — the rasterizer
/// must add the sampled fog color to the lit color, not blend it.
#[derive(Clone, Debug)]
pub struct FogLut {
    /// Normalized view direction the LUT was built around (Y-up).
    pub view_dir: Vec3,
    /// Normalized sun direction (Y-up).
    pub sun: Vec3,
    /// Unit directions, `dirs[j * 5 + i]`: `i` = yaw offset index,
    /// `j` = pitch offset index (offsets -2..2 x 15 degrees).
    pub dirs: [Vec3; 25],
    /// Log-spaced distances 50..20000 m.
    pub dists: [f32; 24],
    /// `values[j * 5 + i][k]` = atmosphere radiance at `dirs[j*5+i]`,
    /// `dists[k] * 0.2` depth.
    pub values: [[[f32; 3]; 24]; 25],
}

impl FogLut {
    pub const DIRS_SIDE: usize = 5;
    pub const DIST_STEPS: usize = 24;
    pub const MIN_DIST: f32 = 50.0;
    pub const MAX_DIST: f32 = 20000.0;
    /// 15 degrees between direction samples (offsets span +-30 degrees).
    pub const DIR_STEP_RAD: f32 = std::f32::consts::PI / 12.0;

    /// Builds the LUT around `view_dir` for a sun direction (both normalized
    /// defensively). 600 atmosphere evaluations — microseconds.
    pub fn build(view_dir: Vec3, sun_yup: Vec3) -> Self {
        let view = view_dir.normalized();
        let sun = sun_yup.normalized();
        let theta0 = view.z.atan2(view.x);
        let phi0 = view.y.clamp(-1.0, 1.0).asin();
        let step = Self::DIR_STEP_RAD;

        let ratio = (Self::MAX_DIST / Self::MIN_DIST).ln();
        let mut dists = [0.0f32; Self::DIST_STEPS];
        for (k, d) in dists.iter_mut().enumerate() {
            *d = Self::MIN_DIST * (ratio * k as f32 / (Self::DIST_STEPS - 1) as f32).exp();
        }

        let mut dirs = [Vec3::ZERO; 25];
        let mut values = [[[0.0f32; 3]; Self::DIST_STEPS]; 25];
        for j in 0..Self::DIRS_SIDE {
            let phi = (phi0 + (j as f32 - 2.0) * step).clamp(-FRAC_PI_2, FRAC_PI_2);
            let cp = phi.cos();
            for i in 0..Self::DIRS_SIDE {
                let theta = theta0 + (i as f32 - 2.0) * step;
                let dir = Vec3::new(cp * theta.cos(), phi.sin(), cp * theta.sin());
                dirs[j * Self::DIRS_SIDE + i] = dir;
                for (k, d) in dists.iter().enumerate() {
                    values[j * Self::DIRS_SIDE + i][k] = atmosphere_yup(dir, sun, d * 0.2);
                }
            }
        }
        FogLut {
            view_dir: view,
            sun,
            dirs,
            dists,
            values,
        }
    }

    /// Trilinear sample: bilinear over the 4 nearest directions (clamped
    /// outside the +-30 degree cone), log-distance interpolation between the
    /// 2 bracketing distances; `dist` is clamped into `[50, 20000]` m.
    ///
    /// The returned color is LINEAR and ADDITIVE (see the struct docs).
    pub fn sample(&self, dir_yup: Vec3, dist: f32) -> [f32; 3] {
        let d = dir_yup.normalized();
        let dist = dist.clamp(Self::MIN_DIST, Self::MAX_DIST);

        let theta = d.z.atan2(d.x);
        let phi = d.y.clamp(-1.0, 1.0).asin();
        let theta0 = self.view_dir.z.atan2(self.view_dir.x);
        let phi0 = self.view_dir.y.clamp(-1.0, 1.0).asin();

        // yaw difference wrapped to (-pi, pi]
        let mut dyaw = (theta - theta0).rem_euclid(TAU);
        if dyaw > PI {
            dyaw -= TAU;
        }
        let step = Self::DIR_STEP_RAD;
        let du = (dyaw / step + 2.0).clamp(0.0, (Self::DIRS_SIDE - 1) as f32);
        let dv = ((phi - phi0) / step + 2.0).clamp(0.0, (Self::DIRS_SIDE - 1) as f32);
        let i0 = (du.floor() as usize).min(Self::DIRS_SIDE - 1);
        let fi = du - i0 as f32;
        let i1 = (i0 + 1).min(Self::DIRS_SIDE - 1);
        let j0 = (dv.floor() as usize).min(Self::DIRS_SIDE - 1);
        let fj = dv - j0 as f32;
        let j1 = (j0 + 1).min(Self::DIRS_SIDE - 1);

        // log-spaced distance interpolation
        let t = ((dist.ln() - Self::MIN_DIST.ln())
            / (Self::MAX_DIST.ln() - Self::MIN_DIST.ln())
            * (Self::DIST_STEPS - 1) as f32)
            .clamp(0.0, (Self::DIST_STEPS - 1) as f32);
        let k0 = (t.floor() as usize).min(Self::DIST_STEPS - 1);
        let fk = t - k0 as f32;
        let k1 = (k0 + 1).min(Self::DIST_STEPS - 1);

        let at = |j: usize, i: usize, k: usize| self.values[j * Self::DIRS_SIDE + i][k];
        let near = lerp3(
            lerp3(at(j0, i0, k0), at(j0, i1, k0), fi),
            lerp3(at(j1, i0, k0), at(j1, i1, k0), fi),
            fj,
        );
        let far = lerp3(
            lerp3(at(j0, i0, k1), at(j0, i1, k1), fi),
            lerp3(at(j1, i0, k1), at(j1, i1, k1), fi),
            fj,
        );
        lerp3(near, far, fk)
    }
}

// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sun;
    use std::sync::OnceLock;

    fn noon_sun() -> Vec3 {
        sun::sun_dir(12.0 * sun::HOUR)
    }

    fn sky() -> &'static SkyLut {
        static LUT: OnceLock<SkyLut> = OnceLock::new();
        LUT.get_or_init(|| SkyLut::build(noon_sun()))
    }

    fn dir_at(azimuth: f32, elevation: f32) -> Vec3 {
        Vec3::new(
            elevation.cos() * azimuth.cos(),
            elevation.sin(),
            elevation.cos() * azimuth.sin(),
        )
    }

    fn assert_close(a: f32, b: f32, rel: f32, what: &str) {
        let tol = rel * (1.0 + a.abs().max(b.abs()));
        assert!(
            (a - b).abs() <= tol,
            "{what}: {a} vs {b} (tol {tol})"
        );
    }

    #[test]
    fn sky_horizon_brighter_than_zenith_and_blue() {
        let lut = sky();
        let s = noon_sun().normalized();
        let az = s.z.atan2(s.x);
        let horizon = lut.sample(dir_at(az, 0.02));
        let zenith = lut.sample(dir_at(0.0, FRAC_PI_2));
        for c in 0..3 {
            assert!(horizon[c].is_finite() && horizon[c] > 0.0);
            assert!(zenith[c].is_finite() && zenith[c] > 0.0);
            assert!(
                horizon[c] > zenith[c],
                "channel {c}: horizon {} <= zenith {}",
                horizon[c],
                zenith[c]
            );
        }
        // Rayleigh sky is blue at the zenith (kRlh blue 22.4e-6 >> red 5.5e-6)
        assert!(zenith[2] > zenith[0]);
    }

    #[test]
    fn sky_lut_has_no_nan_anywhere() {
        let lut = sky();
        let mut j = 0;
        while j < SkyLut::HEIGHT {
            let mut i = 0;
            while i < SkyLut::WIDTH {
                for v in lut.data[j * SkyLut::WIDTH + i] {
                    assert!(v.is_finite(), "NaN/inf at cell ({i}, {j})");
                }
                i += 37;
            }
            j += 11;
        }
        // a sweep of samples too (exercises pole clamp + seam wrap)
        for k in 0..200 {
            let a = k as f32 * 0.713;
            let e = (k as f32 * 0.331).cos(); // -1..1
            for v in lut.sample(dir_at(a, e)) {
                assert!(v.is_finite());
            }
        }
    }

    #[test]
    fn sky_lut_sample_matches_build_at_cell_centers() {
        let lut = sky();
        for (i, j) in [(0usize, 0usize), (128, 64), (200, 100), (17, 3), (255, 127)] {
            let dir = SkyLut::cell_dir(i, j);
            let built = lut.data[j * SkyLut::WIDTH + i];
            let sampled = lut.sample(dir);
            for c in 0..3 {
                assert_close(
                    sampled[c],
                    built[c],
                    1e-3,
                    &format!("cell ({i}, {j}) ch{c}"),
                );
            }
        }
    }

    #[test]
    fn sky_lut_is_continuous_across_longitude_seam() {
        let lut = sky();
        let a = lut.sample(dir_at(PI - 0.001, 0.3));
        let b = lut.sample(dir_at(-PI + 0.001, 0.3));
        for c in 0..3 {
            assert_close(a[c], b[c], 0.05, &format!("seam ch{c}"));
        }
    }

    #[test]
    fn sun_disc_peaks_exactly_toward_sun() {
        let s = noon_sun().normalized();
        // toward the sun the smoothstep gate is exactly 1.0, so the additive
        // radiance is max(s.y, 0) * 10000
        let expected = s.y.max(0.0) * 10000.0;
        assert_close(sun_disc(s, s), expected, 1e-5, "disc at sun");
        // ~2.9 degrees off: dot < 0.99993 -> gate exactly 0
        let off = dir_at(s.z.atan2(s.x) + 0.05, s.y.clamp(-1.0, 1.0).asin());
        assert_eq!(sun_disc(s, off), 0.0);
        // at night (sun below horizon) there is no disc
        let night = sun::sun_dir(0.0);
        assert_eq!(sun_disc(night, night.normalized()), 0.0);
    }

    #[test]
    fn background_gradient_values() {
        // horizon: t_gradient = 0, smoothstep(-0.3, 0.1, 0) = 0.84375
        let h = background_light(Vec3::new(1.0, 0.0, 0.0));
        let f = 0.4 * smoothstep(-0.3, 0.1, 0.0);
        assert_close(h[0], 0.0116 * f, 1e-5, "horizon r");
        assert_close(h[1], 0.027 * f, 1e-5, "horizon g");
        assert_close(h[2], 0.0423 * f, 1e-5, "horizon b");
        // zenith: dark blue, full strength
        let z = background_light(Vec3::new(0.0, 1.0, 0.0));
        assert_close(z[0], 0.0036 * 0.4, 1e-5, "zenith r");
        assert_close(z[1], 0.013 * 0.4, 1e-5, "zenith g");
        assert_close(z[2], 0.0194 * 0.4, 1e-5, "zenith b");
        // below the horizon: black (and never NaN — see the sqrt clamp note)
        let d = background_light(Vec3::new(0.0, -1.0, 0.0));
        assert_eq!(d, [0.0; 3]);
    }

    #[test]
    fn tonemap_zero_and_plateau() {
        assert_eq!(tonemap([0.0; 3]), [0.0; 3]);
        let plateau = 1.830796 / 1.24068; // ~1.4757
        let big = tonemap([1.0e8; 3]);
        for c in big {
            assert!(
                (c - plateau).abs() < 1e-2,
                "plateau {c} vs {plateau}"
            );
        }
        // monotonic between
        assert!(tonemap([0.5; 3])[0] < tonemap([5.0; 3])[0]);
    }

    #[test]
    fn autoexposure_day_and_night() {
        let noon = noon_sun().normalized().y;
        assert_eq!(autoexposure(noon), 1.0); // day -> no boost
        assert_eq!(autoexposure(-0.2), 11.0); // deep night -> x11
        assert_eq!(autoexposure(-0.05), 6.0); // smoothstep(0,0.1,0.05) = 0.5
        assert_eq!(autoexposure(0.0), 1.0); // exactly at the horizon
    }

    #[test]
    fn dither_is_deterministic_and_tiny() {
        let mut distinct = std::collections::HashSet::new();
        for py in 0..64u32 {
            for px in 0..64u32 {
                let d = dither(px, py);
                assert!(d > -0.5 / 255.0 - 1e-6 && d < 0.5 / 255.0 + 1e-6);
                assert_eq!(d, dither(px, py), "not deterministic at ({px}, {py})");
                distinct.insert((d * 255.0 * 1000.0) as i64);
            }
        }
        assert!(distinct.len() > 32, "dither hash collapsed");
    }

    #[test]
    fn fog_at_20km_behaves_like_the_shader() {
        let sun = noon_sun();
        let s = sun.normalized();
        // 45 degrees up, 90 degrees in azimuth away from the sun
        let dir = dir_at(s.z.atan2(s.x) + FRAC_PI_2, FRAC_PI_2 * 0.5);
        let fog = FogLut::build(dir, sun);
        let sky = sky();

        let f20 = fog.sample(dir, 20000.0);
        let f50 = fog.sample(dir, 50.0);
        let f2k = fog.sample(dir, 2000.0);
        let skyc = sky.sample(dir);

        // exactness: the LUT stores the very call fog.wgsl makes
        let expected = atmosphere_yup(dir, sun, 20000.0 * 0.2);
        for c in 0..3 {
            assert!(f20[c].is_finite());
            assert_close(f20[c], expected[c], 1e-4, &format!("fog exact ch{c}"));
            // the 0.4/0.8 aerial damping keeps fog strictly below the sky
            assert!(
                f20[c] <= skyc[c] + 1e-6,
                "fog {c} {} above sky {}",
                f20[c],
                skyc[c]
            );
            assert!(
                f20[c] >= 0.03 * skyc[c],
                "fog {c} {} far below 3% of sky {} (damping broken?)",
                f20[c],
                skyc[c]
            );
        }
        // monotone in distance (approaches the damped sky value)
        assert!(f50[2] < f2k[2] && f2k[2] < f20[2]);
    }

    #[test]
    fn fog_lut_center_cell_is_exact() {
        let sun = noon_sun();
        let dir = dir_at(1.1, 0.4);
        let fog = FogLut::build(dir, sun);

        // center cell (i=2, j=2) at an exact stored distance
        let center = fog.dirs[2 * FogLut::DIRS_SIDE + 2];
        let d12 = fog.dists[12];
        let expected = atmosphere_yup(center, sun, d12 * 0.2);
        for c in 0..3 {
            assert_close(
                fog.values[2 * FogLut::DIRS_SIDE + 2][12][c],
                expected[c],
                1e-6,
                &format!("center build ch{c}"),
            );
            assert_close(
                fog.sample(center, d12)[c],
                expected[c],
                1e-3,
                &format!("center sample ch{c}"),
            );
        }
        // distances are log-spaced 50..20000
        assert_close(fog.dists[0], FogLut::MIN_DIST, 1e-5, "d0");
        assert_close(fog.dists[23], FogLut::MAX_DIST, 1e-4, "d23");
        // dist clamping: anything outside maps to the end samples
        assert_eq!(fog.sample(dir, 0.0), fog.sample(dir, 50.0));
        assert_eq!(fog.sample(dir, 1.0e9), fog.sample(dir, 20000.0));
        // no NaN over a sweep
        for k in 0..50 {
            let a = 0.37 * k as f32;
            let e = (0.11 * k as f32).sin();
            let v = fog.sample(dir_at(a, e), 100.0 * k as f32 + 1.0);
            for c in v {
                assert!(c.is_finite());
            }
        }
    }

    #[test]
    fn atmosphere_matches_reference_implementation() {
        // Independent f64 reimplementation of atmosphere.wgsl for a few
        // directions; verifies the f32 port structurally.
        //
        // NOTE on the tolerance: the analytic roots of `rsi` subtract two
        // ~1.3e7 quantities (`sqrt(d) - b`) to get the ~1e5 m atmosphere
        // chord, amplifying f32 rounding to ~1e-3 relative — the WGSL itself
        // runs in f32 and carries exactly this error. The f64 reference is
        // the "true" value, so port-vs-reference agreement is only expected
        // at the percent level (observed max ~0.8%); anything structural
        // (wrong phase, wrong step) would be off by tens of percent.
        fn reference(r: [f64; 3], s: [f64; 3], max_depth: f64) -> [f64; 3] {
            let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let r0 = [0.0, 0.0, 6371500.0];
            let rsi = |r0: [f64; 3], rd: [f64; 3], sr: f64| -> (f64, f64) {
                let a = dot(rd, rd);
                let b = 2.0 * dot(rd, r0);
                let c = dot(r0, r0) - sr * sr;
                let d = b * b - 4.0 * a * c;
                if d < 0.0 {
                    return (1e5, -1e5);
                }
                (
                    (-b - d.sqrt()) / (2.0 * a),
                    (-b + d.sqrt()) / (2.0 * a),
                )
            };
            let mut p = rsi(r0, r, 6471e3);
            if p.0 > p.1 {
                return [0.0; 3];
            }
            p.1 = rsi(r0, r, 6371e3).0;
            let mut i_dist = max_depth.min(p.1 - p.0);
            if r[2] < 0.0 {
                i_dist = (1000.0 / -r[2]).min(i_dist);
            }
            let i_step = i_dist / 12.0;
            let mut i_time = i_step * 0.375;
            let mut tr = [0.0f64; 3];
            let mut tm = [0.0f64; 3];
            let mut iod_r = 0.0;
            let mut iod_m = 0.0;
            let mu = dot(r, s);
            let gg = 0.758 * 0.758;
            let p_r = 3.0 / (16.0 * std::f64::consts::PI) * (1.0 + mu * mu);
            let p_m = 3.0 / (8.0 * std::f64::consts::PI) * ((1.0 - gg) * (mu * mu + 1.0))
                / ((1.0 + gg - 2.0 * mu * 0.758).powf(1.5) * (2.0 + gg));
            let k_r = [5.5e-6f64, 13.0e-6, 22.4e-6];
            for _ in 0..12 {
                let ip = [
                    r0[0] + r[0] * i_time,
                    r0[1] + r[1] * i_time,
                    r0[2] + r[2] * i_time,
                ];
                let ih = ip[0].hypot(ip[1]).hypot(ip[2]) - 6371e3;
                let od_r = (-ih / 8e3).exp() * i_step;
                let od_m = (-ih / 1.2e3).exp() * i_step;
                iod_r += od_r;
                iod_m += od_m;
                let j_step = rsi(ip, s, 6471e3).1 / 4.0;
                let mut j_time = 0.0;
                let mut jod_r = 0.0;
                let mut jod_m = 0.0;
                for _ in 0..4 {
                    let off = j_time + j_step * 0.5;
                    let jp = [ip[0] + s[0] * off, ip[1] + s[1] * off, ip[2] + s[2] * off];
                    let jh = jp[0].hypot(jp[1]).hypot(jp[2]) - 6371e3;
                    jod_r += (-jh / 8e3).exp() * j_step;
                    jod_m += (-jh / 1.2e3).exp() * j_step;
                    j_time += j_step;
                }
                for c in 0..3 {
                    let attn = (-(21e-6 * (iod_m + jod_m) + k_r[c] * (iod_r + jod_r))).exp();
                    tr[c] += od_r * attn;
                    tm[c] += od_m * attn;
                }
                i_time += i_step;
            }
            let mut out = [0.0f64; 3];
            for c in 0..3 {
                out[c] = 22.0 * (p_r * k_r[c] * tr[c] + p_m * 21e-6 * tm[c]);
            }
            out
        }

        let sun = noon_sun();
        let sz = swap_yz(sun.normalized());
        let s = [sz.x as f64, sz.y as f64, sz.z as f64];
        for (az, el) in [(0.7f32, 0.3f32), (2.5, 0.9), (4.2, 0.05)] {
            let dzup = swap_yz(dir_at(az, el).normalized());
            let r = [dzup.x as f64, dzup.y as f64, dzup.z as f64];
            let port = atmosphere_yup(dir_at(az, el), sun, AtmosphereParams::SKY_DEPTH);
            let reference = reference(r, s, 3.40282347e38);
            for c in 0..3 {
                // background term is identical (same clamp), scattering f64 vs f32
                assert_close(port[c], reference[c] as f32, 2e-2, &format!("ref ch{c}"));
            }
        }
    }
}
