//! roads — the Godot-Road-Generator (GRG) road model, in METERS.
//!
//! Like godot-road-generator, road SURFACES are procedural: RoadPoints
//! carry prior/next handle magnitudes that shape a cubic Bezier per
//! segment; the tessellated curve is swept with a cross-section (lanes,
//! shoulders) to build the road mesh at runtime. GLB is NEVER the road —
//! it is only the prop-asset channel (see roadkit).
//!
//! Scene space: x east, z south, y up (meters). Bearings are radians in
//! the xz plane: direction = (cos θ, sin θ).

pub mod curve;
pub mod junction;
pub mod lane;
pub mod mesh3d;
pub mod model;
pub mod prefab;
pub mod surface;
pub mod terrain;
pub mod tanzania;

pub use curve::{bez_point, flatten_cubic, segment_cubic, CubicSeg, EPS};
pub use junction::{
    marking_parts_for, mesh_parts, mesh_parts_for, roundabout_marking_parts, weld_endpoints,
    Junction, JunctionKind, JunctionLeg, JunctionSettings,
};
pub use lane::{check_lanes, find_nearest_lane, generate_lanes, LaneReport, RoadLane};
pub use mesh3d::{build_parts, gutter_parts, MeshPart, MeshQuality};
pub use model::{Alignment, LaneDir, RoadCurve, RoadPoint, TrafficUpdate};
pub use prefab::{collapse as wfc_collapse, PrefabPlacement};
pub use surface::*;
pub use terrain::TerrainState;
