//! Water shading math — CPU port of the Egregoria engine's
//! `assets/shaders/water.frag.wgsl` (https://github.com/uriopass/egregoria,
//! MIT): the 5-wave Gerstner normal table, the 32 m shore fade, and the
//! final water color composition.
//!
//! # Coordinate frames
//! The shader is Z-up: water waves live in the world `xy` plane (horizontal)
//! and the wave normal's `.z` is up. Here Y is up (roads convention), so
//! waves live in our `xz` plane: [`gerstner_wave_normal`] takes `p = (x, z)`
//! (the shader passes `wpos.xy * 0.01`, i.e. horizontal position scaled by
//! 0.01 — do the same) and returns the normal as
//! `(shader.x, shader.z, shader.y)` = `(east, up, south-ish)`, the same y/z
//! swap used by [`crate::atmosphere`] / [`crate::sun`].
//!
//! # Documented deviations / substitutions
//! - The shader perturbs the Gerstner normal with two scrolling noise
//!   textures (`t_wavy`) before normalizing; we have no textures, so
//!   [`gerstner_wave_normal`] returns the raw table result and
//!   [`gerstner_wave_normal_unit`] is the drop-in replacement for the
//!   shader's `normalize(normal + wavy * 0.15 + wavy2 * 0.1)`.
//! - The shader evaluates `atmosphere(reflected, sun, 1e38)` inline for the
//!   reflection; the CPU rasterizer supplies that as the `sky_reflection`
//!   argument (e.g. [`crate::atmosphere::SkyLut::sample`] of the mirrored
//!   direction), so [`water_color`] takes the reflection precomputed.
//! - The shader adds fog to the LINEAR color and tonemaps once
//!   (`final_rgb += fog; final_rgb = tonemap(final_rgb)`); [`water_color`]
//!   tonemaps internally (per the port spec), so renderers that add fog in
//!   linear space should use [`water_color_linear`] and tonemap after adding
//!   the fog. [`water_color`] == [`crate::atmosphere::tonemap`](
//!   [`water_color_linear`]`(...)`).

use crate::atmosphere::tonemap;
use gdmath::Vec3;

/// Water base color from `water.frag.wgsl` (`vec3(0.262, 0.396, 0.508)`),
/// linear sRGB.
pub const WATER_BASE: [f32; 3] = [0.262, 0.396, 0.508];

/// The `relative_water_depth = clamp(water_depth / 32.0, 0, 1)` divisor from
/// `water.frag.wgsl` — full water tint 32 m deep.
pub const SHORE_FADE_M: f32 = 32.0;

/// `(wavelength, amplitude, dir_x, dir_y)` — the 5-wave table from
/// `water.frag.wgsl`, verbatim. `dir` is horizontal (shader `xy` = our
/// `xz` plane).
const WAVES: [(f32, f32, f32, f32); 5] = [
    (2.0, 0.05, 0.0, -1.0),
    (4.0, 0.0125, 0.0, 1.0),
    (6.0, 0.0037, 1.0, 1.0),
    (2.0, 0.008, 1.0, 1.0),
    (0.8, 0.001, -0.5, 0.3),
];

/// Gerstner wave normal from `water.frag.wgsl::gerstnerWaveNormal`, exact
/// port (`speed 0.5`, `steepness 0.3`, `Qi / WA / phi / rad` formulas as in
/// the shader).
///
/// * `p` — horizontal sample position `(x, z)` in world meters PRE-SCALED by
///   0.01 (the shader calls `gerstnerWaveNormal(wpos.xy * 0.01, time)`; its
///   `xy` horizontal plane is our `xz`).
/// * `t` — seconds (`params.time_always`).
///
/// Returns the UN-normalized normal `(x, y_up, z)` — the shader adds noise
/// perturbations before its own normalize; use
/// [`gerstner_wave_normal_unit`] when you want the shader's final
/// (noise-free) result.
pub fn gerstner_wave_normal(p: (f32, f32), t: f32) -> (f32, f32, f32) {
    const N_WAVES: f32 = 5.0;
    const SPEED: f32 = 0.5;
    const STEEPNESS: f32 = 0.3;
    // shader normal starts at (0, 0, 1) with .z up -> here (x, y_up=1, z)
    let mut normal = (0.0f32, 1.0f32, 0.0f32);
    for &(length, amplitude, dir_x, dir_y) in &WAVES {
        let wi = 2.0 / length;
        let wa = wi * amplitude;
        let phi = SPEED * wi;
        let rad = wi * (dir_x * p.0 + dir_y * p.1) + phi * t;
        let qi = STEEPNESS / (amplitude * wi * N_WAVES);
        let c_dir = wa * rad.cos();
        // shader: normal.x -= dir.x * c_dir; normal.y -= dir.y * c_dir;
        //         normal.z -= Qi * WA * sin(rad)
        normal.0 -= dir_x * c_dir;
        normal.2 -= dir_y * c_dir; // shader .y (horizontal) -> our z
        normal.1 -= qi * wa * rad.sin(); // shader .z (up) -> our y
    }
    normal
}

/// [`gerstner_wave_normal`] taken to length 1 — the CPU stand-in for the
/// shader's `normalize(normal + wavy * 0.15 + wavy2 * 0.1)` (the scrolling
/// noise textures are not ported).
pub fn gerstner_wave_normal_unit(p: (f32, f32), t: f32) -> (f32, f32, f32) {
    let n = gerstner_wave_normal(p, t);
    let l = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
    if l > 1e-9 {
        (n.0 / l, n.1 / l, n.2 / l)
    } else {
        (0.0, 1.0, 0.0)
    }
}

/// Shore fade from `water.frag.wgsl`:
/// `relative_water_depth = clamp(depth / 32, 0, 1)`,
/// `base_factor = mix(0.03, 0.3, pow(1.0 - relative_water_depth, 4.0))`.
///
/// Deep water (>= [`SHORE_FADE_M`]) -> 0.03 (nearly pure reflection);
/// the shoreline -> 0.3 (strong body color).
pub fn shore_factor(water_depth_m: f32) -> f32 {
    let relative = (water_depth_m / SHORE_FADE_M).clamp(0.0, 1.0);
    0.03 + (0.3 - 0.03) * (1.0 - relative).powi(4)
}

/// `reflect_coeff = 1.0 - dot(V, normal)` from `water.frag.wgsl` (V = unit
/// vector from the surface TOWARD the camera, normal = unit wave normal,
/// Y-up). Grazing view -> 1.0, looking straight down -> 0.0.
pub fn water_reflect_coeff(view_dir: Vec3, normal: (f32, f32, f32)) -> f32 {
    let v = view_dir.normalized();
    let n = {
        let l = (normal.0 * normal.0 + normal.1 * normal.1 + normal.2 * normal.2).sqrt();
        if l > 1e-9 {
            (normal.0 / l, normal.1 / l, normal.2 / l)
        } else {
            (0.0, 1.0, 0.0)
        }
    };
    1.0 - (v.x * n.0 + v.y * n.1 + v.z * n.2)
}

/// Linear (pre-tonemap) water color — the exact `final_rgb` lines of
/// `water.frag.wgsl` before fog and tonemap:
/// `base_color + sunpower * reflected_atmo` with
/// `base_color = base_factor * vec3(0.262, 0.396, 0.508)` and
/// `sunpower = 0.1 * reflect_coeff`.
///
/// * `sky_reflection` — linear HDR sky radiance of the mirrored direction;
///   the shader computes `atmosphere(reflected, sun, 1e38)` inline, the CPU
///   rasterizer supplies e.g. [`crate::atmosphere::SkyLut::sample`] of the
///   reflected direction (see [`water_reflected_dir`]).
/// * `normal`, `view_dir`, `sun` are kept for shader-parameter parity; the
///   shader computes a specular term from them (`pow(clamp(dot(R,V),0,1),5)`)
///   but does NOT add it to `final_rgb` (it only feeds the alpha), and the
///   reflection arrives precomputed — hence the `let _ =`.
///
/// Note: add fog to this linear result, THEN tonemap (see the module docs).
pub fn water_color_linear(
    normal: (f32, f32, f32),
    view_dir: Vec3,
    sun: Vec3,
    sky_reflection: [f32; 3],
    base_factor: f32,
    reflect_coeff: f32,
) -> [f32; 3] {
    let _ = (normal, view_dir, sun);
    let sunpower = 0.1 * reflect_coeff;
    [
        base_factor * WATER_BASE[0] + sunpower * sky_reflection[0],
        base_factor * WATER_BASE[1] + sunpower * sky_reflection[1],
        base_factor * WATER_BASE[2] + sunpower * sky_reflection[2],
    ]
}

/// Final water color = [`water_color_linear`] passed through
/// [`tonemap`] — the tail of `water.frag.wgsl` (`final_rgb = tonemap(...)`,
/// fog excepted; add fog in linear space via [`water_color_linear`] if you
/// need the shader's exact fog ordering).
pub fn water_color(
    normal: (f32, f32, f32),
    view_dir: Vec3,
    sun: Vec3,
    sky_reflection: [f32; 3],
    base_factor: f32,
    reflect_coeff: f32,
) -> [f32; 3] {
    tonemap(water_color_linear(
        normal,
        view_dir,
        sun,
        sky_reflection,
        base_factor,
        reflect_coeff,
    ))
}

/// The mirrored view direction `reflect(-V, normal)` from
/// `water.frag.wgsl` (V = toward camera), unit length, Y-up — the direction
/// to sample the sky LUT at for `sky_reflection`.
pub fn water_reflected_dir(view_dir: Vec3, normal: (f32, f32, f32)) -> Vec3 {
    let v = view_dir.normalized();
    let n = {
        let l = (normal.0 * normal.0 + normal.1 * normal.1 + normal.2 * normal.2).sqrt();
        if l > 1e-9 {
            (normal.0 / l, normal.1 / l, normal.2 / l)
        } else {
            (0.0, 1.0, 0.0)
        }
    };
    // reflect(I, N) = I - 2*dot(N, I)*N with I = -V  ->  2*dot(V,N)*N - V
    let vn = v.x * n.0 + v.y * n.1 + v.z * n.2;
    Vec3::new(2.0 * vn * n.0 - v.x, 2.0 * vn * n.1 - v.y, 2.0 * vn * n.2 - v.z).normalized()
}

/// The fragment alpha of `water.frag.wgsl`: `0.9 + reflect_coeff * 0.1`.
pub fn water_alpha(reflect_coeff: f32) -> f32 {
    0.9 + reflect_coeff * 0.1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::atmosphere::{atmosphere_yup, SkyLut};
    use crate::sun;
    use std::sync::OnceLock;

    fn noon_sun() -> Vec3 {
        sun::sun_dir(12.0 * sun::HOUR)
    }

    fn sky() -> &'static SkyLut {
        static LUT: OnceLock<SkyLut> = OnceLock::new();
        LUT.get_or_init(|| SkyLut::build(noon_sun()))
    }

    fn close(a: f32, b: f32, tol: f32, what: &str) {
        assert!((a - b).abs() <= tol, "{what}: {a} vs {b} (tol {tol})");
    }

    #[test]
    fn gerstner_rest_normal_matches_hand_computed_table() {
        // At p=(0,0), t=0 every rad == 0, so cos = 1 / sin = 0 and the normal
        // is (0,0,1) minus sum(dir * WA) in the horizontal axes.
        // Hand-computed from the table: nx = -0.00798333, up = 1, nz = 0.03376667.
        let n = gerstner_wave_normal((0.0, 0.0), 0.0);
        close(n.0, -0.00798333, 1e-6, "rest nx");
        close(n.1, 1.0, 1e-6, "rest up");
        close(n.2, 0.03376667, 1e-6, "rest nz");
        // near-up but not exactly unit (the caller/shader normalizes)
        let l = (n.0 * n.0 + n.1 * n.1 + n.2 * n.2).sqrt();
        assert!((l - 1.0).abs() < 0.01);
        let u = gerstner_wave_normal_unit((0.0, 0.0), 0.0);
        let lu = (u.0 * u.0 + u.1 * u.1 + u.2 * u.2).sqrt();
        close(lu, 1.0, 1e-6, "unit length");
    }

    #[test]
    fn gerstner_waves_move_and_stay_finite() {
        let a = gerstner_wave_normal((3.7, -1.2), 10.0);
        let b = gerstner_wave_normal((3.7, -1.2), 25.0);
        assert_ne!(a, b, "waves must animate with t");
        for v in [a.0, a.1, a.2, b.0, b.1, b.2] {
            assert!(v.is_finite());
        }
        // steepness term engages over time (up component leaves 1.0)
        assert!(a.1 < 1.0);
    }

    #[test]
    fn shore_factor_matches_base_factor_formula() {
        close(shore_factor(0.0), 0.3, 1e-6, "shore at 0 m");
        close(shore_factor(-5.0), 0.3, 1e-6, "clamped shallow");
        close(shore_factor(32.0), 0.03, 1e-6, "deep at 32 m");
        close(shore_factor(100.0), 0.03, 1e-6, "clamped deep");
        // 16 m: 0.03 + 0.27 * (1 - 0.5)^4 = 0.03 + 0.016875
        close(shore_factor(16.0), 0.046875, 1e-6, "half depth");
    }

    #[test]
    fn water_color_math_end_to_end() {
        let sun = noon_sun();
        let view = Vec3::new(0.3, 0.9, -0.2); // toward camera
        let normal = gerstner_wave_normal_unit((1.3, -0.7), 4.0);
        let rc = water_reflect_coeff(view, normal);
        assert!((0.0..=1.0).contains(&rc));

        // sky_reflection = SkyLut sample of the mirrored direction (the
        // documented substitute for the shader's inline atmosphere call)
        let refl = water_reflected_dir(view, normal);
        assert!((refl.len() - 1.0).abs() < 1e-5);
        let sky_reflection = sky().sample(refl);
        // mirror of a downward view over an up-facing normal looks upward
        assert!(refl.y > 0.0);

        let base = shore_factor(20.0);
        let linear = water_color_linear(normal, view, sun, sky_reflection, base, rc);
        for c in 0..3 {
            let expected = base * WATER_BASE[c] + 0.1 * rc * sky_reflection[c];
            close(linear[c], expected, 1e-6, &format!("linear ch{c}"));
        }
        let tonemapped = water_color(normal, view, sun, sky_reflection, base, rc);
        let tm = tonemap(linear);
        for c in 0..3 {
            close(tonemapped[c], tm[c], 1e-6, &format!("final ch{c}"));
        }
        // alpha from the shader's return line
        close(water_alpha(rc), 0.9 + rc * 0.1, 1e-6, "alpha");
    }

    #[test]
    fn reflect_coeff_grazing_vs_nadir() {
        let up = (0.0f32, 1.0f32, 0.0f32);
        // straight down at the surface: V.N = 1 -> rc = 0
        close(water_reflect_coeff(Vec3::new(0.0, 1.0, 0.0), up), 0.0, 1e-6, "nadir");
        // grazing: V.N = 0 -> rc = 1
        close(water_reflect_coeff(Vec3::new(1.0, 0.0, 0.0), up), 1.0, 1e-6, "grazing");
    }

    #[test]
    fn water_sky_reflection_links_to_atmosphere() {
        // the substitution claim: SkyLut::sample(reflected) approximates the
        // shader's inline atmosphere(reflected, sun, 1e38)
        let sun = noon_sun();
        let view = Vec3::new(0.1, 0.6, 0.4);
        let n = gerstner_wave_normal_unit((0.4, 0.9), 2.0);
        let refl = water_reflected_dir(view, n);
        let via_lut = sky().sample(refl);
        let direct = atmosphere_yup(refl, sun, crate::atmosphere::AtmosphereParams::SKY_DEPTH);
        for c in 0..3 {
            close(via_lut[c], direct[c], 0.02, &format!("reflection ch{c}"));
        }
    }
}
