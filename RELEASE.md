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
