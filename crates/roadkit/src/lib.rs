//! roadkit — GLB prop assets + Godot-style prop joining + the software 3D scene.
//!
//! Architecture mirrors godot-road-generator exactly:
//! - road surfaces: PROCEDURAL (roads::mesh3d sweeps the RoadPoint curve)
//!   and TEXTURED with the plugin's trim sheet (LaneType UV columns)
//! - GLB/glTF: ONLY roadside props (vehicle, tree, streetlight, milestone,
//!   junctionMarker) attached along the curve with distance + lateral offset
//! - rendering: dependency-free CPU rasterizer → egui texture

pub mod bridge_host;
pub mod export;
pub mod glb;
pub mod library;
pub mod props;
pub mod raster;
pub mod scene;
pub mod traffic_sim;
pub mod textures;
pub mod traffic;

pub use glb::{parse_glb, GltfAsset, GltfPrim};
pub use library::{AssetEntry, AssetLibrary};
pub use props::{join_along, stamp, JoinParams, PropInstance};
pub use scene::{
    build_scene, build_scene_with_bg, render_scene, render_scene_env, render_scene_props,
    render_scene_props_env, Env, ENV_DAY, PropDraw, Scene, ScenePart,
};
pub use traffic_sim::{SimSettings, TrafficSim};
pub use textures::{Texture, TextureLibrary};
