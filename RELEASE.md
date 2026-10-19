traffic-sim v0.2.0
===================

New in v0.2.0 (vs v0.1.0 baseline)
---------------------------------
- Fresh render set under Xvfb + lavapipe (screenshots/v02kijazi_*.png, screenshots/v02igesa_*.png)
- Published versioned gallery: public/traffic-sim/v0.2.0/ (12 shots)
- Downloadable source bundle: download/traffic-sim.zip (also public/download/traffic-sim.zip)
- Upstream sources pinned as git submodules under _sources/: godot-road-generator,
  Traffic-Toolkit-DOTS, Mapkit, egregoria

Data / features carried from v0.1.0
-----------------------------------
- Kijazi flyover (Ubungo, Dar es Salaam): 176 roads, 6 bridges with piers, BRT cement decks,
  deck + below-deck intersections visible
- Igesa Rd x Sam Nujoma Rd (Sinza): 545 curves, 678 junctions, Mapbox MVT + terrarium DEM
- Traffic lights derived from junction legs (216/216 heads bound 1:1 - intersection/light mismatch fixed)
- roads crate: gutters, roundabout lane divisions, surface types (asphalt/cement/concrete/cobble/gravel/mud),
  per-sample elevation
- tsdata: OSM/Overpass + Mapbox (streets MVT, terrain-rgb, terrarium) providers with hardcoded token
  extracted from uploaded mapkit-rs/mapstudio-rs zips

Build
-----
  cargo check --workspace   # engine crate needs vulkan dev libs (see /home/z/work/env.sh)
  cargo run -p yakui-app -- --data kijazi --shots 3

## v0.2.1

- Real-world detail pass on the native bridge system (zoomed cameras):
  on-deck drone + low-hover shots along the span, under-deck views standing on
  the lower intersection, pier close-ups, wide context — cameras derived from
  the longest bridge's own geometry (deck centroid, exact deck_y, farthest-pair
  span axis, ground-aware eye heights clamped to the clearance envelope)
- Crisp deck paint: solid white edge lines + geometric dashed lane lines
  (1.6 m on / 3.2 m period) on both carriageways; BRT median decks get
  per-carriageway dashes (BridgeSpec.median_m)
- Real-world sky: gradient above the horizon replaces the flat clear color
  (background pixels only — geometry reaching above the horizon is preserved);
  haze-colored clear fills voids beyond the terrain square
- tsdata assets_data_dir(): TRAFFIC_SIM_ASSETS env override + CWD fallback so
  prebuilt release binaries find the asset tree anywhere
- CI: .github/workflows/build.yml — release builds for Linux + Windows
  (build-only, no Xvfb/display in CI), artifacts on every push, GitHub Release
  with binaries on v* tags
- 16 fresh renders at 1600 px in screenshots/ (Kijazi + Igesa)

## v0.2.2

- 3D viewport grid remake (the "environment" pass):
  - adaptive spacing 10/25/50/100 m by scene radius, major line every 4th
  - per-vertex distance fade toward the viewport backdrop — far lines melt
    into the void instead of aliasing into dashed speckle (the CPU raster
    has no MSAA; the old full-length uniform quads broke up into dots)
  - origin axes tinted X-red / Z-blue like the viewport axis gizmo
  - faint radial stage disc under the grid (rim = exact backdrop, seamless)
    so roads no longer float in a pure void
  - per-vertex color path in the rasterizer (ScenePart.vcolors) + explicit
    backdrop plumbing: build_scene_with_bg / build_scene_full_with_bg so
    the roadkit-cli (#1a1c20) and the editor (#1e222a) both get seamless fades
- verified: egui-app --selftest ALL PASS, --uitest 246/246, new reference
  renders screenshots/v022{editor_grid,ramps_grid,city_grid}.png
