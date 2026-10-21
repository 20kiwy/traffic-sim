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
//! - `atmosphere`: the sky/fog/tonemap math — a CPU port of egregoria's
//!   WGSL shaders (`atmosphere.wgsl` single-scattering sky with the 12x4
//!   ray-march, the painted background gradient + sun disc, `tonemap.wgsl`,
//!   the autoexposure/dither from `pbr/render.wgsl`, and the aerial
//!   perspective of `fog.wgsl`), plus the `SkyLut` (256x128 lat-long linear
//!   sky cache) and `FogLut` (5x5 dirs x 24 log-dists) the software
//!   rasterizer consumes. Z-up WGSL -> Y-up conversion happens inside the
//!   module (same y/z swap as `sun_dir`).
//! - `water`: Gerstner wave normals, 32 m shore fade and the water color
//!   composition from `water.frag.wgsl`.

pub mod atmosphere;
pub mod pem;
pub mod sun;
pub mod terrain;
pub mod time;
pub mod tools;
pub mod water;

pub use atmosphere::{
    atmosphere_yup, autoexposure, background_light, dither, sun_disc, tonemap, AtmosphereParams,
    FogLut, SkyLut,
};
pub use pem::{Heightmap, Pem, CHUNK_SIZE, CHUNKS_PER_SIDE, RESOLUTION};
pub use sun::{sun_color, sun_dir, sun_dir_yup, up_shade, ShadowQuality, SunState};
pub use terrain::TerrainBaker;
pub use terrain::SquareStage;
pub use time::GameTime;
pub use tools::{Tool, TOOLS};
pub use water::{
    gerstner_wave_normal, gerstner_wave_normal_unit, shore_factor, water_color,
    water_color_linear, water_reflect_coeff, water_reflected_dir, SHORE_FADE_M, WATER_BASE,
};
pub use pem::TerraformKind;
