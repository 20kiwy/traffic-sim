//! Prop joining — the godot-road-generator attachment model.
//!
//! Props (vehicles, milestones, trees, streetlights, junction markers)
//! are attached to a RoadCurve with a distance-along + lateral offset;
//! vehicles/streetlights auto-orient along the road tangent like a GRG
//! RoadPointAttachment. GLB is only the asset container.

use gdmath::vec3::Vec3;
use roads::model::RoadCurve;

/// A prop instance placed in the scene.
#[derive(Clone, Debug)]
pub struct PropInstance {
    /// asset id from the roadkit manifest ("vehicle", "tree", ...)
    pub asset: String,
    /// world position (meters; y = elevation)
    pub pos: Vec3,
    /// yaw around Y (radians) — tangent-aligned when auto-joined
    pub yaw: f64,
    pub scale: f64,
    pub label: String,
    /// name of the curve that owns this prop (for per-curve clearing)
    pub owner: String,
}

#[derive(Clone, Debug)]
pub struct JoinParams {
    /// distance between props along the curve (meters)
    pub spacing: f64,
    /// lateral offset from the centerline (meters, sign flips when alternating)
    pub lateral: f64,
    /// alternate sides every prop (streetlight/tree style)
    pub alternate: bool,
    /// yaw-align the prop with the road tangent (vehicle/streetlight style)
    pub align: bool,
    pub scale: f64,
    /// random spacing/scale jitter 0..1
    pub jitter: f64,
    /// random yaw for non-aligned props (trees)
    pub random_yaw: bool,
    pub seed: u64,
}

impl Default for JoinParams {
    fn default() -> Self {
        Self {
            spacing: 30.0,
            lateral: 6.0,
            alternate: true,
            align: true,
            scale: 1.0,
            jitter: 0.0,
            random_yaw: false,
            seed: 1337,
        }
    }
}

/// Deterministic LCG (no external rand dependency).
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next_f64(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (self.0 >> 33) as f64 / (1u64 << 31) as f64
    }
}

/// Right-of-travel lateral vector for a yaw in (x, z) with x east / z south.
fn right_of(yaw: f64) -> (f64, f64) {
    (yaw.sin(), -yaw.cos())
}

/// Place a run of props along a curve — the "join like in Godot" action.
pub fn join_along(curve: &RoadCurve, asset: &str, p: &JoinParams) -> Vec<PropInstance> {
    let mut rng = Lcg::new(p.seed);
    let mut out: Vec<PropInstance> = Vec::new();
    let length = curve.length_m();
    if length < 4.0 || p.spacing <= 0.0 {
        return out;
    }
    let mut side = 1.0f64;
    let mut s = rng.next_f64() * p.spacing.min(length * 0.25).max(1.0);
    while s <= length {
        if let Some((pos, yaw)) = curve.sample_at(s) {
            let (rx, rz) = right_of(yaw);
            let lateral = p.lateral * side;
            let jitter_scale = 1.0 + (rng.next_f64() - 0.5) * 2.0 * p.jitter;
            let y = pos.y + if asset == "vehicle" { 0.08 } else { 0.0 };
            let prop_yaw = if p.align {
                yaw
            } else if p.random_yaw {
                rng.next_f64() * std::f64::consts::TAU
            } else {
                0.0
            };
            out.push(PropInstance {
                asset: asset.to_string(),
                pos: Vec3::new(pos.x + (rx * lateral) as f32, y as f32, pos.z + (rz * lateral) as f32),
                yaw: prop_yaw,
                scale: p.scale * jitter_scale,
                label: String::new(),
                owner: curve.name.clone(),
            });
        }
        side = if p.alternate { -side } else { side };
        let step = p.spacing * (1.0 + (rng.next_f64() - 0.5) * 2.0 * p.jitter);
        s += step.max(1.0);
    }
    out
}

/// Stamp a single prop at normalized curve position `t` (0..1).
pub fn stamp(
    curve: &RoadCurve,
    asset: &str,
    t: f64,
    lateral: f64,
    align: bool,
    scale: f64,
) -> Option<PropInstance> {
    let length = curve.length_m();
    if length <= 0.0 {
        return None;
    }
    let (pos, yaw) = curve.sample_at(t.clamp(0.0, 1.0) * length)?;
    let (rx, rz) = right_of(yaw);
    let y = pos.y + if asset == "vehicle" { 0.08 } else { 0.0 };
    Some(PropInstance {
        asset: asset.to_string(),
        pos: Vec3::new(pos.x + (rx * lateral) as f32, y as f32, pos.z + (rz * lateral) as f32),
        yaw: if align { yaw } else { 0.0 },
        scale,
        label: String::new(),
        owner: curve.name.clone(),
    })
}

/// Per-asset 2D marker colors (screen-space prop icons).
pub fn asset_marker_color(asset_id: &str) -> [u8; 3] {
    match asset_id {
        "vehicle" => [217, 119, 6],
        "tree" => [47, 158, 99],
        "streetlight" => [234, 179, 8],
        "milestone" => [148, 163, 184],
        "junctionMarker" => [220, 38, 38],
        _ => [168, 162, 158],
    }
}
