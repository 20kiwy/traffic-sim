//! roadkit — GLB prop assets + Godot-style prop joining + the software 3D scene.
//!
//! Architecture mirrors godot-road-generator exactly:
//! - road surfaces: PROCEDURAL (roads::mesh3d sweeps the RoadPoint curve)
//!   and TEXTURED with the plugin's trim sheet (LaneType UV columns)
//! - GLB/glTF: ONLY roadside props (vehicle, tree, streetlight, milestone,
//!   junctionMarker) attached along the curve with distance + lateral offset
//! - rendering: dependency-free CPU rasterizer → egui texture

pub mod bridge_host;
pub mod dsm;
pub mod export;
pub mod glb;
pub mod library;
pub mod props;
pub mod raster;
pub mod scene;
pub mod signals;
pub mod traffic_sim;
pub mod textures;
pub mod traffic;

pub use glb::{parse_glb, GltfAsset, GltfPrim};
pub use library::{AssetEntry, AssetLibrary};
pub use dsm::{build_dsm, build_dsm_local, DsmScene};
pub use props::{join_along, stamp, JoinParams, PropInstance};
pub use scene::{
    build_scene, build_scene_with_bg, render_scene, render_scene_env, render_scene_props,
    render_scene_props_env, Env, ENV_DAY, PropDraw, Scene, ScenePart,
};
pub use signals::{
    Control, GroupTiming, SignalEntry, SignalPlan, SignalRegistry, SignalState,
    preset_two_phase, preset_tazara_4phase, preset_ubungo_2phase, preset_ubungo_4phase,
};
pub use traffic_sim::{LightPose, SimSettings, TrafficSim};
pub use textures::{Texture, TextureLibrary};
