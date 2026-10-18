//! engine — the egregoria-derived engine port for traffic-sim.
//!
//! Copied from https://github.com/uriopass/egregoria (MIT), adapted:
//! - `sun`: the analytic day/night sun (game_loop.rs `manage_gfx_params` formula),
//!   portable to the software rasterizer's Lambert light.
//! - `pem`: the Procedural Elevation Module — a DEM updater that feeds sampled
//!   terrain heights (terrarium / Mapbox terrain-RGB) into the packed heightmap
//!   chunks, then stamps road/bridge overrides (egregoria `height_override`).
//! - `terrain`: terrain baking (egregoria `HeightmapRender` ported to CPU):
//!   chunk mip + normal bake, then the SquareStage — the single square ground
//!   patch that replaces egregoria's streamed terrain chunks (the square shows
//!   the roads; there are no terrain chunks to stream).
//! - `tools`: the land toolbars, hardcoded as a lib — the Tool enum, the
//!   toolbox icon table, terraform kinds and the ImmediateDraw overlay orders,
//!   with the exact HUD layout (64 px icon buttons on blur panels) copied from
//!   `native_app/src/gui/hud/toolbox/mod.rs`.

pub mod pem;
pub mod sun;
pub mod terrain;
pub mod time;
pub mod tools;

pub use pem::{Heightmap, Pem, CHUNK_SIZE, CHUNKS_PER_SIDE, RESOLUTION};
pub use sun::{sun_color, sun_dir, ShadowQuality, SunState};
pub use terrain::TerrainBaker;
pub use terrain::SquareStage;
pub use time::GameTime;
pub use tools::{Tool, TOOLS};
pub use pem::TerraformKind;
