//! The sun — analytic day/night cycle copied from egregoria
//! (`native_app/src/game_loop.rs::manage_gfx_params`, MIT).
//!
//! Pure math, no GPU: the same `sun` / `sun_col` that egregoria uploads into
//! `RenderParams`, computed from a time-of-day in seconds. The software
//! rasterizer consumes it directly as the Lambert light direction/color.

use gdmath::Vec3;

/// Seconds in a full day (egregoria `GameTime::DAY`).
pub const DAY: f64 = 86400.0;
/// Seconds in an hour.
pub const HOUR: f64 = 3600.0;
/// The hour the day starts at (egregoria starts day 1 at 8:00).
pub const DAY_START_H: f64 = 8.0;

/// Direction of the sun at `time_of_day` seconds.
///
/// Port of egregoria:
/// ```text
/// let t = TAU * (time - 8 * HOUR) / DAY;
/// let sun = vec3(t.cos(), t.sin() * 0.5, t.sin() + 0.5).normalize();
/// ```
/// In egregoria's frame Z is up; here Y is up (roads crate convention), so we
/// swap the components — the `y` component is the sun's height above the
/// horizon (positive = daylight).
pub fn sun_dir(time_of_day: f64) -> Vec3 {
    let t = std::f64::consts::TAU * (time_of_day - DAY_START_H * HOUR) / DAY;
    let s = Vec3::new(t.cos() as f32, (t.sin() + 0.5) as f32, (t.sin() * 0.5) as f32);
    // egregoria (x, y, z_up) = (cos, sin+0.5, sin*0.5) -> here (east, up, north)
    Vec3::new(s.x, s.z, s.y)
}

/// Sun color (linear), from egregoria's `sun_col` formula:
/// `4 * sqrt(sqrt(z.max(0))) * (1, 0.95 + z*0.05, 0.95 + z*0.05)`.
/// Returns rgba premultiplied by intensity, values 0..~4.
pub fn sun_color(dir: Vec3) -> [f32; 4] {
    let z = dir.y.max(0.0);
    let i = 4.0 * (z as f32).sqrt().sqrt();
    [
        i,
        i * (0.95 + 0.05 * z as f32),
        i * (0.95 + 0.05 * z as f32),
        1.0,
    ]
}

/// A flat rgba color (0..1) for rasterizers that clamp their light to 0..1.
pub fn sun_light_flat(time_of_day: f64) -> [f32; 4] {
    let d = sun_dir(time_of_day);
    let c = sun_color(d);
    let m = c[0].max(c[1]).max(c[2]).max(1.0);
    let clamp = |v: f32| (v / m).clamp(0.08, 1.0);
    [clamp(c[0]), clamp(c[1]), clamp(c[2]), 1.0]
}

/// Shadow map quality ladder copied from egregoria's `ShadowQuality`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ShadowQuality {
    NoShadows,
    Low512,
    Medium1024,
    High2048,
    Ultra4096,
}

impl ShadowQuality {
    pub fn resolution(self) -> u32 {
        match self {
            ShadowQuality::NoShadows => 0,
            ShadowQuality::Low512 => 512,
            ShadowQuality::Medium1024 => 1024,
            ShadowQuality::High2048 => 2048,
            ShadowQuality::Ultra4096 => 4096,
        }
    }
}

/// Snapshot of everything a renderer needs to light the scene.
#[derive(Copy, Clone, Debug)]
pub struct SunState {
    /// normalized direction TOWARD the sun
    pub dir: Vec3,
    /// linear color x intensity (can exceed 1 at noon)
    pub col: [f32; 4],
    /// time of day in seconds
    pub time_of_day: f64,
    /// shadow map setting
    pub shadows: ShadowQuality,
}

impl SunState {
    pub fn at(time_of_day: f64) -> Self {
        let dir = sun_dir(time_of_day);
        SunState {
            dir,
            col: sun_color(dir),
            time_of_day,
            shadows: ShadowQuality::Medium1024,
        }
    }

    /// Lambert factor for a world-space normal, 0.35 ambient floor
    /// (matches the software rasterizer's `0.38 + 0.62 * max(n·l, 0)`).
    pub fn lambert(&self, normal: Vec3) -> f32 {
        let n = normal.normalized();
        let d = n.dot(self.dir).max(0.0) as f32;
        0.38 + 0.62 * d
    }
}

#[cfg(test_mod)]
mod tests_mod {}
