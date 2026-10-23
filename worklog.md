
---
Task ID: 2-b
Agent: Z.ai Code (main)
Task: User: "check the map launcher repo copy full 100% to the ergorogeria so the carto to be 3d to show well road junctions well lanes well all things well make it well" (+ screenshot showing pink dots on a green plane)

Work Log:
- Identified "map launcher" = 20kiwy/map-launcher (pure-Rust openstreetmap-carto tile server for Dodoma; crates: carto-math, carto-style, carto-core, tile-server*)
- VENDORED 1:1 into apps/egregoria: carto-math + carto-style crates (MSS parser, 55-role palette registry, embedded verbatim assets/style/*.mss (19 sheets), symbols (12 MB on-disk), patterns, DejaVu fonts) + clone/map-launcher as 9th reference repo
- maptools/src/carto.rs NEW: CartoStyle over Palette::from_embedded(); land/water/landcover/buidling/road fill+casing per OSM class exactly per roads.mss + tile-server casing_for/fill_for; @bridge-casing black; oneway arrow = darken(casing,20); road_class_rank for junction fills; 2 unit tests pin the documented hexes (land #f2efe9, water #aad3df, motorway #e892a2, building #d9d0c9, primary #fcd6a4) — PASS
- CartoData.parks -> areas: Vec<(@var, poly)>; overpass.rs now classifies landuse/natural/leisure/amenity into carto @vars (26 areas on kijazi); residential halo derived from real building footprints (x1.8) since the extract has no landuse polys
- cartography.rs: ground = @land-color full tile; carto paint order landuse -> landcover -> water; z-stack with per-poly epsilon; bridge railings recolored
- map_mesh.rs: per-road carto RoadStyle via RoadProps.highway — casing stroke (width+2.4) at z-0.06, fill at z-0.03, lane strips (driving=fill, walking=@pedestrian-fill, parking=fill*0.85), separators = casing, junction surface = most-major connected road fill, walking corners = pedestrian fill + junction casing, bridges = black casing, oneway arrows tinted darken(casing); buildings get @building-line outline strokes around footprints; houses_mesh material switched palette.png -> tess (pink texel root cause #2)
- colors.lua data-driven: roof=@building-line #bcb3aa, house=@building-fill #d9d0c9, lots=@residential, sand/sea = carto @sand/@water-color
- NOON start (GameTime forced to 12:00) — the 08:00 low sun tinted every wall pink
- ROOT CAUSES of the "pink dots" (user screenshot) SOLVED: (1) houses_mesh sampled palette.png at uv[0,0] (pink texel), (2) THE BIG ONE: hud::power_errors floats the red "no_power" icon over EVERY building of a blackout network — the OSM import has no producers so all 3257 buildings showed it (the "donuts" survived every 3D-draw bisect because they are yakui SCREEN-SPACE widgets); gated on the OSM mode
- Environment trees/lamps gated off for the OSM mode (no terrain = no trees/lamps; standing "no trees" request); flat carto shading: shadows/SSAO off (shadow-map acne ripples + blue-noise moire on the flat quad), fog off (washed the map at 1.5 km)
- carto_drop: the flat carto stack sinks by 1.5x the reversed-z depth quantum (d^2/2^24) so the 0.2 m road-vs-ground separation never ties on GLES (whole road grid was losing the depth test at fitted zoom); EGRE_FIT=0 now really skips the fit (was dist=0); path_not_found reroute sprites fade out after 90 ticks; EGRE_CARS env overrides the 150-car spawn (0 = verification mode)
- gen_exterior_house scale 40->26 divisor: OSM buildings now fill their real footprints (were ~40%)
- Verification: cargo check clean; maptools tests 2/2; Xvfb :99 live-frame captures (md5-diff liveness loop — the sandbox resets + llvmpipe GL kills made naive captures stale; final loop verifies hash change + settled frame): fitted tile shot + 300 m close-up; screenshots copied to my-project/public/egregoria-carto3d-{fit,close,junctions}.png
- NOTE for future agents: sandbox resets wipe /home/z/traffic-sim — restore = rustup minimal, git-lfs-3.7.0, ~/.git-credentials, clone 20kiwy/traffic-sim; ALSA needs libasound debs extracted to /tmp/alsalib (PKG_CONFIG_PATH + LIBRARY_PATH); EGL/xkb stack in /tmp/xkbx; llvmpipe kills native_app ~50% of launches right after audio init — retry-loop with md5 liveness checks; an old traffic-sim demo process was painting a ghost 3x3 grid over :99 (killed)

Stage Summary:
- The 3D carto now wears the FULL openstreetmap-carto palette from the map-launcher style engine: per-class road fills+casings (motorway #e892a2/#dc2a67 ... residential #ffffff/#bbb), black bridge casings, carto junction fills, lane strips, walking/parking surfaces, landcover/landuse stack on @land-color, carto buildings (#d9d0c9 + #bcb3aa outlines, footprints filled), noon light, no pink dots, no trees/lamps/fog/acne
- crates: apps/egregoria/carto-style + carto-math (vendored), maptools::carto bridge; commit pending push
