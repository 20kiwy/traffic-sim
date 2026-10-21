# The 3D environment system (R15 — Egregoria port)

The 3D environment is a CPU port of the [Egregoria](https://github.com/uriopass/egregoria)
engine's forward pipeline (MIT), flattened onto the software rasterizer in
`crates/roadkit/src/raster.rs`. The math lives in `crates/engine`
(`atmosphere.rs`, `water.rs`, `sun.rs`, `time.rs`) — all pure, tested, no GPU.

## What a frame does (env path)

1. **Sun** — analytic day cycle (`engine::sun`): `t = τ·(time−8h)/24`,
   `sun = (cos t, sin t + 0.5, −sin t·0.5)` (y-up),
   `sun_col = 4·sun.y^¼ · (1, 0.95+0.05·sun.y, 0.95+0.05·sun.y)`.
2. **Sky LUT** — `SkyLut::build(sun)` evaluates the single-scattering
   atmosphere (`atmosphere.wgsl`: 12×4 ray-march, iSun 22, g 0.758) into a
   256×128 lat-long cache — rebuilt only when the sun moves.
3. **Fog LUT** — per camera: `FogLut::build(view_dir, sun)` = 5×5 dirs × 24
   log-dists of aerial-perspective samples (`maxDepth = dist·0.2`, the WGSL
   damping) — the CPU fog prepass; per fragment it is trilinearly sampled
   and added (`ambient + Lo + fog`, exactly Egregoria's combine).
4. **Shadow pass** — one ortho cascade (1024² native / 512² wasm) around the
   camera target; parts + props cast, ground receives. Slope-scaled bias,
   3×3 PCF, soft 0.35 floor (ambient never reads pure black).
5. **Raster (linear)** — albedo² (gamma-2.0), `0.05 ambient +
   sun_col·(n·l)·shadow`, optional contours (10 m / 50 m, fwidth-faded),
   water = Gerstner 5-wave normals + TEX_WAVY perturbation + sky-LUT
   reflection + z-buffer shore fade (32 m).
6. **Compose** — sky pixels (depth-unwritten) = sky LUT + analytic sun disc;
   then Egregoria's autoexposure (×10 at night), tonemap
   `1.83c/(1.24c+1.68)`, hash dither, gamma-2.0 encode.

## Controls / knobs

| Where | What |
|---|---|
| Editor toolbar | `sun` time-of-day slider (5–21 h), `shadows`, `contours`, `water` toggles |
| CLI | `ROADKIT_TIME` (hours), `ROADKIT_SHADOWS=0`, `ROADKIT_CONTOURS=1`, `ROADKIT_WATER=<m>`, `ROADKIT_NOFOG=1` (diag) |
| Camera | Egregoria orbit feel: target-damped yaw 16 / pitch 8 / dist 8, wheel `1.05^(0.5+0.1·|w|)`, dist clamp [5, 1500] |

## Ground

Both grounds (flat + terrain heightfield) are uniform grids textured with
the procedural `terrain_grass` albedo (200 m world tiling), tinted by the
engine grass ramp, blended to sand/sea below the waterline, and faded into
the haze at the rim. The old 3-ring fan (wedge-striped) is gone; the
flatten follows the LOWEST road envelope at interchanges so underpass
roads are never buried.

## R16 — the editor look (default) + atmosphere toggle

The full atmosphere is now a TOGGLE (`sky` in the editor toolbar,
`ROADKIT_ATMO` in the CLI; `Environment::new(..., atmo, backdrop)`). The
DEFAULT is the clean editor look the user asked for ("full grid, no green
background, no whitish cloud"):

- **No aerial-perspective fog, no sky LUT fill, no sun disc** — sky pixels
  are a flat backdrop (`Environment.backdrop`, the editor theme's input
  color), pre-inverted through autoexposure + tonemap so the encoded
  pixels match the surrounding UI paper exactly (`editor_backdrop_linear`).
- **No grass ground** — the terrain/grass mesh is skipped and replaced by
  one unlit backdrop-colored floor quad (`Frame::flat_floor`, y = −0.02,
  extent `editor_extent = max(radius·3, 500 m, dist·3)`), so floor and
  backdrop melt into one seamless paper with no visible horizon.
- **Full grid** — the TerraZone overlay grid rides the same editor extent
  (not a 28-cell patch), with a zoom-scaled window (up to 320 cells) and a
  gentle far fade; ink switches with the theme (light paper → black ink,
  dark charcoal → white ink). Overlay text (HUD, point labels, tooltips,
  ruler) follows the theme too.
- Turning `sky` on restores the full R15 environment (atmosphere LUTs,
  grass ground, sun shadows, water, contours) for pretty renders.

## R16 — scene origin (the alignment fix)

`Scene` now carries `origin: [f32; 2]` — the bbox-centre recentering that
`build_scene_enriched` subtracts from every mesh vertex. Document-space
overlays (RoadPoint gizmos, signal heads, turn labels, cursor pick, the
AddPoint write-back) and the per-frame traffic/flow sim draws convert
through it (doc → scene: `p − origin`; scene → doc: `p + origin`), so
gizmos stay glued to the meshes for scenes authored anywhere in the world
and points placed in 3D land at the right document coordinates.

## R16 — band-render aspect fix

`draw_all` computed the projection aspect from `frame.h` — the BAND height
in multithreaded rendering — squeezing every multi-band render to
`(1/threads)` width about the viewport centre (2 bands → the mesh rendered
at ~50 % width while the screen-space overlays stayed correct: the
"points and render not well aligned" report). The aspect now uses
`frame.full_h`.
