//! gdmath — minimal math for the RoadKit Godot engine.
//!
//! Scene space is METERS: x east, y up, z south (top-down plan view reads
//! like a map: x right, z down). Zero external dependencies.

pub mod mat4;
pub mod vec2;
pub mod vec3;

pub use mat4::{Mat4, OrbitCam};
pub use vec2::Vec2;
pub use vec3::Vec3;
