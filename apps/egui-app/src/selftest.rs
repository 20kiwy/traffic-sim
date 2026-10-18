//! Headless end-to-end verification (`roadkit-godot --selftest`).
//! Exercises the whole pipeline without a display:
//! GLB assets → .gd parse → .gd run (script-driven scene) → procedural
//! road meshes → prop join → software 3D render → pixel sanity.

use crate::bridge::{self, SceneState};
use crate::scripts;
use gdmath::OrbitCam;
use roadkit::{build_scene, render_scene, AssetLibrary, TextureLibrary};
use std::cell::RefCell;
use std::rc::Rc;

fn fail(msg: String) -> i32 {
    println!("SELFTEST FAIL: {msg}");
    1
}

pub fn run() -> i32 {
    println!("== RoadKit Godot selftest ==");

    // 1. assets
    let dir = crate::state::find_assets_dir();
    let mut lib = AssetLibrary::load_dir(&dir);
    if lib.entries.is_empty() {
        return fail(format!("no GLB assets found under {:?}", dir));
    }
    println!("assets: {} loaded from {:?}", lib.entries.len(), dir.join("glb"));
    for e in &lib.entries {
        println!("  {:<16} {:>4} tris  ({})", e.id, e.mesh.triangle_count(), e.role);
    }
    if lib.entries.len() < 5 {
        return fail(format!("expected >= 5 assets, got {}", lib.entries.len()));
    }
    lib.render_thumbs(64);
    let thumbs_ok = lib
        .entries
        .iter()
        .all(|e| e.thumb.as_ref().map(|t| t.len() == 64 * 64 * 4).unwrap_or(false));
    if !thumbs_ok {
        return fail("thumbnail rendering produced wrong buffer size".into());
    }

    // 1b. textures (road trim sheet fetched from the plugin + generated)
    let textures = TextureLibrary::load_dir(&dir.join("textures"));
    if textures.entries.len() < 9 {
        return fail(format!(
            "expected >= 9 textures (road_trim/grass/sidewalk/median/asphalt/cement/mud/rough/red_tactile), got {}",
            textures.entries.len()
        ));
    }
    for (name, t) in [
        ("road_trim", &textures.entries[0]),
        ("grass", &textures.entries[1]),
        ("sidewalk", &textures.entries[2]),
        ("median", &textures.entries[3]),
        ("asphalt", &textures.entries[4]),
        ("cement", &textures.entries[5]),
        ("mud", &textures.entries[6]),
        ("rough", &textures.entries[7]),
        ("red_tactile", &textures.entries[8]),
    ] {
        println!("texture: {:<10} {}x{}", name, t.w, t.h);
    }
    // R7 texture checks: cement is light grey, mud is warm brown, tactile is red
    let cement = &textures.entries[5];
    let mud = &textures.entries[6];
    let tactile = &textures.entries[8];
    let cs = cement.sample(0.5, 0.5);
    let ms = mud.sample(0.5, 0.5);
    let ts = tactile.sample(0.5, 0.5);
    if !(cs[0] > 0.45 && cs[0] < 0.85 && (cs[0] - cs[2]).abs() < 0.12) {
        return fail(format!("cement texture not light grey: {cs:?}"));
    }
    if !(ms[0] > ms[1] && ms[1] > ms[2]) {
        return fail(format!("mud texture not warm brown: {ms:?}"));
    }
    if !(ts[0] > 0.4 && ts[0] > ts[1] && ts[1] > ts[2]) {
        return fail(format!("red_tactile not red: {ts:?}"));
    }
    println!(
        "texture: cement L={:.2} mud rgb=({:.2},{:.2},{:.2}) tactile rgb=({:.2},{:.2},{:.2}) OK",
        (cs[0] + cs[1] + cs[2]) / 3.0,
        ms[0], ms[1], ms[2],
        ts[0], ts[1], ts[2]
    );
    // sample checks: the double-yellow lives at u≈0.5 (FAST right edge +
    // TWO_WAY left edge), SLOW's solid white at u≈0.1155, plain at u≈0.97
    let trim = &textures.entries[0];
    let ys = trim.sample(0.4925, 0.5); // left line of the double yellow
    let ws = trim.sample(0.1155, 0.5); // SLOW solid white edge line
    let gs = trim.sample(0.97, 0.5); // plain asphalt column
    println!(
        "texture: trim samples — yellow rgb=({:.2},{:.2},{:.2}) white rgb=({:.2},{:.2},{:.2}) plain rgb=({:.2},{:.2},{:.2})",
        ys[0], ys[1], ys[2], ws[0], ws[1], ws[2], gs[0], gs[1], gs[2]
    );
    if !(ys[0] > 0.6 && ys[1] > 0.45 && ys[2] < 0.45) {
        return fail("road trim sheet: expected double-yellow at the u=0.5 seam".into());
    }
    if !(ws[0] > 0.7 && ws[1] > 0.7 && ws[2] > 0.7) {
        return fail("road trim sheet: expected SLOW solid white edge line at u≈0.1155".into());
    }

    // 2. parse all bundled scripts
    for (label, file, src) in scripts::all() {
        match gdscript::parse(src) {
            Ok(_) => println!("parse: {label} ({file}) OK"),
            Err(e) => return fail(format!("parse {file}: {e}")),
        }
    }

    // 3. run each script on a fresh scene and check the pipeline
    //    (label, file, min curves, min props)
    let expectations: [(&str, &str, usize, usize); 6] = [
        ("Demo Highway", "demo_highway.gd", 1, 8),
        ("Ring Loop", "ring_loop.gd", 1, 6),
        ("Grid Network", "grid_network.gd", 5, 6),
        ("Junction Test", "junction_test.gd", 5, 0),
        ("Traffic Junction", "traffic_junction.gd", 2, 8),
        ("BRT Mwendokasi", "brt_corridor.gd", 1, 4),
    ];

    for (label, file, min_curves, min_props) in expectations {
        let src = scripts::all()
            .into_iter()
            .find(|(_, f, _)| *f == file)
            .map(|(_, _, s)| s)
            .unwrap_or("");
        let script = match gdscript::parse(src) {
            Ok(s) => s,
            Err(e) => return fail(format!("re-parse {file}: {e}")),
        };
        let scene: Rc<RefCell<SceneState>> = Rc::new(RefCell::new(SceneState::default()));
        let mut it = gdscript::Interpreter::new();
        bridge::register(&mut it, &scene);
        if let Err(e) = it.run(&script, "main") {
            return fail(format!("run {file}: {e}"));
        }
        let out = it.take_output();
        for l in &out {
            println!("  | {l}");
        }
        let sc = scene.borrow();
        if sc.curves.len() < min_curves {
            return fail(format!(
                "{file}: expected >= {min_curves} curves, got {}",
                sc.curves.len()
            ));
        }
        if sc.props.len() < min_props {
            return fail(format!(
                "{file}: expected >= {min_props} joined GLB props, got {}",
                sc.props.len()
            ));
        }
        println!(
            "run: {label} OK — {} curves, {} points, {} junctions, {} GLB props",
            sc.curves.len(),
            sc.curves.iter().map(|c| c.points.len()).sum::<usize>(),
            sc.junctions.len(),
            sc.props.len()
        );

        // junction assertions for the junction test scene:
        // crossing (4 legs) + roundabout (3 legs) + T-junction (3 legs)
        if file == "junction_test.gd" {
            if sc.junctions.len() != 3 {
                return fail(format!(
                    "junction_test: expected 3 junctions (crossing + roundabout + T), got {}",
                    sc.junctions.len()
                ));
            }
            let kinds: Vec<String> = sc
                .junctions
                .iter()
                .map(|j| format!("{}:{}", j.kind.label(), j.legs.len()))
                .collect();
            println!("  junctions detected: {}", kinds.join(", "));
            let has_cross4 = sc
                .junctions
                .iter()
                .any(|j| j.kind == roads::JunctionKind::Crossing && j.legs.len() == 4);
            let has_round = sc.junctions.iter().any(|j| {
                j.kind == roads::JunctionKind::Roundabout
                    && j.legs.len() == 3
                    && (j.island.is_some()
                        || j.prefab
                            .as_ref()
                            .map(|p| p.id.starts_with("roundabout"))
                            .unwrap_or(false))
            });
            let has_t = sc
                .junctions
                .iter()
                .any(|j| j.legs.len() == 3 && j.kind == roads::JunctionKind::Crossing);
            if !has_cross4 {
                return fail("junction_test: no 4-leg crossing detected".into());
            }
            if !has_round {
                return fail("junction_test: no 3-leg roundabout with island detected".into());
            }
            if !has_t {
                return fail("junction_test: no T-junction (3 legs, crossing) detected".into());
            }
            // WFC: the 4-leg crossing should collapse to a hand-modeled tile
            let wfc_tiles: Vec<&str> = sc
                .junctions
                .iter()
                .filter_map(|j| j.prefab.as_ref().map(|p| p.id.as_str()))
                .collect();
            println!("  WFC tiles collapsed: {}", if wfc_tiles.is_empty() { "none".into() } else { wfc_tiles.join(", ") });
            if !wfc_tiles.iter().any(|t| t.starts_with("4way") || t.starts_with("3way")) {
                return fail("junction_test: WFC did not collapse any junction to a GLB prefab".into());
            }
            // traffic sim: vehicles spawn, obey signals, stop lines located
            let mut sim = roadkit::TrafficSim::new(
                &sc.curves,
                &sc.junctions,
                roadkit::SimSettings { density: 40.0, ..Default::default() },
            );
            if sim.vehicles.is_empty() {
                return fail("traffic sim: no vehicles spawned".into());
            }
            let s_before: f64 = sim.vehicles.iter().map(|v| v.s).sum();
            for _ in 0..40 {
                sim.step(0.1, &sc.curves);
            }
            let s_after: f64 = sim.vehicles.iter().map(|v| v.s).sum();
            if (s_after - s_before).abs() < 1.0 {
                return fail("traffic sim: vehicles did not move".into());
            }
            println!(
                "  traffic sim: {} vehicles, moved {:.0} vehicle-m in 4 s, {} stop lines",
                sim.vehicles.len(),
                s_after - s_before,
                sim.stop_line_count()
            );
            // outlines must be non-degenerate
            for j in &sc.junctions {
                if j.outline.len() < 3 {
                    return fail("junction_test: junction outline degenerate".into());
                }
            }
        }

        // traffic pack: the signalized junction must place lights + signs
        if file == "traffic_junction.gd" {
            let light_assets = sc
                .props
                .iter()
                .filter(|p| p.asset.starts_with("trafficLight"))
                .count();
            let sign_assets = sc.props.iter().filter(|p| p.asset.starts_with("sign")).count();
            if light_assets < 3 {
                return fail(format!(
                    "traffic_junction: expected >= 3 traffic lights, got {light_assets}"
                ));
            }
            if sign_assets < 4 {
                return fail(format!("traffic_junction: expected >= 4 road signs, got {sign_assets}"));
            }
            println!("  traffic pack: {light_assets} signal heads + {sign_assets} signs placed");
        }
        // BRT corridor: cement median + median stations
        if file == "brt_corridor.gd" {
            let stations = sc
                .props
                .iter()
                .filter(|p| p.asset.starts_with("brt"))
                .count();
            if stations < 3 {
                return fail(format!("brt_corridor: expected >= 3 BRT assets, got {stations}"));
            }
            let has_brt_median = sc
                .curves
                .iter()
                .any(|c| c.points.iter().any(|p| p.median >= 3.0));
            if !has_brt_median {
                return fail("brt_corridor: no >= 3.0 m (BRT) median on the trunk".into());
            }
            // the built mesh must carry cement median strips + tactile edges
            let built = build_scene(&sc.curves, &sc.junctions, &sc.props, &lib, false);
            let cement_parts = built.parts.iter().filter(|p| p.tex == roadkit::textures::TEX_CEMENT).count();
            let tactile_parts = built.parts.iter().filter(|p| p.tex == roadkit::textures::TEX_TACTILE).count();
            if cement_parts == 0 {
                return fail("brt_corridor: no cement (BRT) median strips in the mesh".into());
            }
            if tactile_parts < 2 {
                return fail(format!(
                    "brt_corridor: expected tactile edge strips on the BRT median, got {tactile_parts}"
                ));
            }
            println!("  brt pack: {stations} station assets, cement strips {cement_parts}, tactile edges {tactile_parts}");
        }

        // 4. build + render a frame (only for the demo, keeps output short)
        if file == "demo_highway.gd" {
            let built = build_scene(&sc.curves, &sc.junctions, &sc.props, &lib, true);
            let stats = built.stats;
            if stats.tris < 250 {
                return fail(format!("road mesh too small: {} tris", stats.tris));
            }
            // textured parts must exist (trim sheet lane strips: one per lane)
            let tex_parts = built.parts.iter().filter(|p| p.tex >= 0).count();
            if tex_parts < 2 {
                return fail(format!(
                    "expected textured lane strips on the road mesh, got {tex_parts}"
                ));
            }
            // grid-only environment: the ground may NOT carry the grass
            // texture any more (Godot-editor style grid, no green field)
            let grass_ground = built
                .ground
                .iter()
                .filter(|p| p.tex == roadkit::textures::TEX_GRASS)
                .count();
            if grass_ground > 0 {
                return fail(format!(
                    "environment must be grid-only — {grass_ground} ground parts still use the grass texture"
                ));
            }
            let cam = OrbitCam { dist: 120.0, pitch: 0.5, yaw: 0.7, ..Default::default() };
            let (w, h) = (640usize, 480usize);
            let rgba = render_scene(&built, &lib, &textures, &cam, w, h, [24, 25, 28, 255], &[]);
            let total = (w * h) as f64;
            let nonbg = rgba
                .chunks_exact(4)
                .filter(|p| !(p[0] == 24 && p[1] == 25 && p[2] == 28))
                .count();
            let pct = nonbg as f64 / total * 100.0;
            println!(
                "render: {} tris, {nonbg}/{total:.0} non-bg pixels ({pct:.1}% coverage, grid-only ground)",
                stats.tris
            );
            if pct < 2.0 {
                return fail(format!("rendered frame nearly empty ({pct:.1}% coverage)"));
            }

            // R10-d: the AI RoadLane graph must generate for the demo scene
            // (total > 0, some connected, NO unexpected dead ends)
            let report = roads::lane::check_lanes(&sc.lanes, &sc.curves);
            if report.total_lanes == 0 {
                return fail("demo_highway: no AI lanes generated for the demo scene".into());
            }
            let connected: usize = report.containers.iter().map(|c| c.connected).sum();
            if connected == 0 {
                return fail("demo_highway: AI lanes generated but none connected".into());
            }
            let unexpected: usize = report
                .containers
                .iter()
                .map(|c| c.unexpected_dead_ends.len())
                .sum();
            if unexpected > 0 {
                return fail(format!(
                    "demo_highway: {unexpected} unexpected lane dead ends"
                ));
            }
            println!(
                "lanes: {} AI RoadLanes on the demo, {connected} connected, 0 unexpected dead ends",
                report.total_lanes
            );
            // R10-d: terrain defaults — the connector is OFF until added
            if sc.terrain.enabled {
                return fail("terrain must default to disabled (enabled=false)".into());
            }
            println!("terrain: defaults OK (enabled=false until a connector is added)");
        }
    }

    // 5. junction engine unit checks (pure geometry, no scripts)
    {
        use roads::junction::{detect, weld_endpoints, JunctionSettings};
        use roads::model::{RoadCurve, RoadPoint};
        let mut s = JunctionSettings::default();
        s.auto_roundabout = false; // crossings only for this check
        // two 200 m straight roads crossing at the origin
        let mk = |name: &str, x1: f64, z1: f64, x2: f64, z2: f64| {
            let mut c = RoadCurve::new(name);
            let mut a = RoadPoint::new(x1, z1);
            a.bearing = (z2 - z1).atan2(x2 - x1);
            a.next_mag = 60.0;
            let mut b = RoadPoint::new(x2, z2);
            b.bearing = a.bearing;
            b.prior_mag = 60.0;
            c.add_point(a);
            c.add_point(b);
            c
        };
        let curves = vec![
            mk("NS", 0.0, -100.0, 0.0, 100.0),
            mk("EW", -100.0, 0.0, 100.0, 0.0),
        ];
        let js = detect(&curves, &s);
        if js.len() != 1 || js[0].legs.len() != 4 {
            return fail(format!(
                "junction detect: expected 1 crossing with 4 legs, got {} junctions",
                js.len()
            ));
        }
        if js[0].outline.len() != 8 {
            return fail(format!(
                "junction detect: 4-way n-gon should have 8 corners, got {}",
                js[0].outline.len()
            ));
        }
        // three roads ending near (50, 50) within snap distance
        let s3 = JunctionSettings::default();
        let curves3 = vec![
            mk("A", -60.0, 0.0, 50.0, 50.0),
            mk("B", 60.0, -20.0, 52.0, 54.0),
            mk("C", 40.0, 140.0, 46.0, 44.0),
        ];
        let jr = detect(&curves3, &s3);
        let roundabout = jr.iter().find(|j| j.kind == roads::JunctionKind::Roundabout);
        match roundabout {
            Some(j) if j.legs.len() == 3 && j.island.is_some() && j.outline.len() == 24 => {
                println!(
                    "junction engine: crossing(4-leg octagon) + roundabout(3-leg ring r={:.1}) OK",
                    j.ring_outer
                );
            }
            other => {
                return fail(format!("junction detect: roundabout wrong: {other:?}"));
            }
        }
        // weld: endpoints ~5 m apart snap to the mean (within the 8 m snap)
        let mut curves_w = vec![mk("W1", 0.0, 0.0, 90.0, 2.0), mk("W2", 95.0, 3.0, 200.0, 0.0)];
        let welded = weld_endpoints(&mut curves_w, 8.0);
        if welded != 1
            || (curves_w[0].points[1].x - curves_w[1].points[0].x).abs() > 1e-9
            || (curves_w[0].points[1].x - 92.5).abs() > 1e-9
        {
            return fail("weld_endpoints: endpoints not welded to the shared mean".into());
        }
        println!("junction engine: endpoint weld OK (mean x = {:.1})", curves_w[0].points[1].x);
    }

    // 6. interpreter edge cases (loops, strings, vectors)
    {
        let src = r#"
extends Node3D

func _ready():
        var acc = 0
        for i in range(10):
                if i % 2 == 0:
                        acc += i
        var v = Vector3(1, 2, 3) * 2
        print("acc=" + str(acc) + " v=" + str(v) + " len=" + str(v.length()))
"#;
        let script = match gdscript::parse(src) {
            Ok(s) => s,
            Err(e) => return fail(format!("edge-case parse: {e}")),
        };
        let scene: Rc<RefCell<SceneState>> = Rc::new(RefCell::new(SceneState::default()));
        let mut it = gdscript::Interpreter::new();
        bridge::register(&mut it, &scene);
        if let Err(e) = it.run(&script, "main") {
            return fail(format!("edge-case run: {e}"));
        }
        let out = it.take_output();
        let joined = out.join(" | ");
        println!("interp: {joined}");
        if !out
            .iter()
            .any(|l| l.contains("acc=20") && l.contains("Vector3(2.0, 4.0, 6.0)"))
        {
            return fail(format!("edge-case output wrong: {joined}"));
        }
    }

    // 7. terrain connector: set_terrain(true) via script, then the scene builds
    {
        let src = r#"
extends Node3D

func _ready():
        set_terrain(true)
        var curve = RoadCurve.new("Hillpass")
        var a = RoadPoint.new()
        a.position = Vector3(-80, 3, 0)
        a.bearing = 0.0
        var b = RoadPoint.new()
        b.position = Vector3(80, 3, 16)
        curve.add_point(a)
        curve.add_point(b)
        curve.build_mesh()
        print("terrain scene built: " + str(curve.length()) + " m")
"#;
        let script = match gdscript::parse(src) {
            Ok(s) => s,
            Err(e) => return fail(format!("terrain script parse: {e}")),
        };
        let scene: Rc<RefCell<SceneState>> = Rc::new(RefCell::new(SceneState::default()));
        let mut it = gdscript::Interpreter::new();
        bridge::register(&mut it, &scene);
        if let Err(e) = it.run(&script, "main") {
            return fail(format!("terrain script run: {e}"));
        }
        let out = it.take_output();
        for l in &out {
            println!("  | {l}");
        }
        let sc = scene.borrow();
        if !sc.terrain.enabled {
            return fail("set_terrain(true) via script did not enable the connector".into());
        }
        let t = &sc.terrain;
        if !(t.amplitude == 6.0
            && t.wavelength == 130.0
            && (t.offset + 0.25).abs() < 1e-9
            && (t.edge_margin - 0.5).abs() < 1e-9
            && (t.edge_falloff - 2.0).abs() < 1e-9
            && t.flatten_under_roads)
        {
            return fail(format!("terrain defaults changed: {t:?}"));
        }
        // the terrain-enabled scene must BUILD (heightfield ground + road mesh)
        let built = roadkit::scene::build_scene_full(
            &sc.curves,
            &sc.junctions,
            &sc.props,
            &sc.lanes,
            &lib,
            true,
            false,
            Some(&sc.terrain),
        );
        if built.ground.is_empty() {
            return fail("terrain-enabled scene built no ground heightfield".into());
        }
        if built.stats.tris == 0 {
            return fail("terrain-enabled scene built an empty road mesh".into());
        }
        println!(
            "terrain: set_terrain(true) via script — ground parts {}, {} tris, road {:.0} m",
            built.ground.len(),
            built.stats.tris,
            sc.curves.iter().map(|c| c.length_m()).sum::<f64>()
        );
    }

    println!(
        "SELFTEST OK — assets, .gd parse/run, GRG meshes, junctions, lanes, terrain, prop join, 3D render, interpreter: ALL PASS"
    );
    0
}
