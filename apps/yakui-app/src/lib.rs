//! yakui-app — the data-driven traffic-sim application with the
//! yakui/goryak HUD (hardcoded egregoria toolbox layout).
//!
//! Pipeline: OSM/Mapbox raw data (tsdata) → procedural roads (roads,
//! the godot-road-generator port) + native bridges (bridge) + DEM-driven
//! terrain (engine PEM + terrain baking) → traffic (toolkit) → software
//! rasterized frames (roadkit) with the yakui HUD (tsui) → PNG screenshots.

use image::RgbaImage;

pub const ASSETS: &str = "assets";

/// Assets root: `TRAFFIC_SIM_ASSETS` env override, else `./assets` (repo run).
fn assets_root() -> std::path::PathBuf {
    match std::env::var("TRAFFIC_SIM_ASSETS") {
        Ok(d) if !d.trim().is_empty() => std::path::PathBuf::from(d),
        _ => std::path::PathBuf::from(ASSETS),
    }
}
pub const OUT_DIR: &str = "screenshots";

pub struct RunConfig {
    pub site: String,      // "kijazi" | "igesa" | script name
    pub out_prefix: String,
    pub size: usize,
    pub ss: usize,
    pub shots: usize,
    pub sim_seconds: f64,
    pub hud: bool,
    pub plan: bool,
}

impl Default for RunConfig {
    fn default() -> Self {
        RunConfig {
            site: "kijazi".into(),
            out_prefix: "traffic_sim".into(),
            size: 1280,
            ss: 2,
            shots: 3,
            sim_seconds: 14.0,
            hud: true,
            plan: true,
        }
    }
}

pub struct Scene {
    pub state: roadkit::bridge_host::SceneState,
    pub library: roadkit::library::AssetLibrary,
    pub textures: roadkit::textures::TextureLibrary,
    pub bridge_parts: Vec<roads::mesh3d::MeshPart>,
    pub bridge_info: Vec<bridge::BridgeBuild>,
    pub ground_parts: Vec<roads::mesh3d::MeshPart>,
    pub toolkit: Option<toolkit::TrafficToolkit>,
    pub report: Vec<String>,
}

/// Build the full data scene for a site ("kijazi" | "igesa") from the local
/// cached raw data (no network needed).
pub fn build_data_scene(site: &str) -> Result<Scene, String> {
    let assets = assets_root();
    let data = assets.join("data");
    let mut imp = match site {
        "kijazi" => tsdata::import::kijazi_demo(&data),
        "igesa" => tsdata::import::igesa_demo(&data),
        other => return Err(format!("unknown data site `{other}` (kijazi|igesa)")),
    }
    .map_err(|e| format!("data import failed: {e}"))?;

    // doc for the projection anchor
    let doc_path = match site {
        "kijazi" => data.join("kijazi_area_roads.json"),
        _ => data.join("igesa_samnujoma.json"),
    };
    let doc = tsdata::osm::load_local(&doc_path).map_err(|e| e.to_string())?;
    let (lat0, lon0) = tsdata::import::projection_center(&doc);
    engine::terrain::set_anchor(lat0, lon0);

    // DEM from the local terrarium tile (z13 4988/4250)
    let dem_path = data.join("mapbox/terrarium_13_4988_4250.png");
    let dem_bytes = std::fs::read(&dem_path).map_err(|e| format!("read {}: {e}", dem_path.display()))?;
    let dem = tsdata::dem::DemGrid::from_terrarium_png_tile(&dem_bytes, 13, 4988, 4250)
        .map_err(|e| e.to_string())?;
    let ground_at = |lat: f64, lon: f64| -> f64 {
        dem.sample(lat, lon).unwrap_or(40.0).clamp(2.0, 250.0) as f64
    };

    // bridges (native system): deck + piers + clearance
    let mut bridge_parts = Vec::new();
    let mut bridge_info = Vec::new();
    let mut top_bridges = imp.bridges.clone();
    top_bridges.sort_by(|a, b| b.nodes.len().cmp(&a.nodes.len()));
    for b in top_bridges.iter().take(6) {
        let display_name = if b.name.is_empty() {
            format!("unnamed {} bridge", b.highway)
        } else {
            b.name.clone()
        };
        let spec = bridge::BridgeSpec::from_tags(
            &display_name,
            &b.bridge,
            b.layer,
            if b.highway == "trunk" || b.highway == "primary" { 3 } else { 2 },
            b.is_brt,
        );
        let built = bridge::build_from_feature(&b.nodes, &spec, |lat, lon| ground_at(lat, lon));
        bridge_info.push(built);
    }
    for b in &bridge_info {
        bridge_parts.extend(b.parts.iter().cloned());
    }

    // PEM: square world covering the imported area, DEM update + road stamps
    let mut min_x = f64::INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for c in &imp.curves {
        for p in &c.points {
            min_x = min_x.min(p.x);
            min_z = min_z.min(p.z);
            max_x = max_x.max(p.x);
            max_z = max_z.max(p.z);
        }
    }
    if !min_x.is_finite() {
        return Err("no curves imported".into());
    }
    let span = (max_x - min_x).max(max_z - min_z);
    let side_chunks = ((span / 512.0).ceil() as u32 + 2).clamp(4, 12);
    let world_side = side_chunks as f64 * 512.0;
    let cx = (min_x + max_x) / 2.0;
    let cz = (min_z + max_z) / 2.0;
    let origin = (cx - world_side / 2.0, cz - world_side / 2.0);
    let mut pem = engine::pem::Pem::with_side(origin, side_chunks);
    let (dwest, dsouth, deast, dnorth) = dem.tile_box();
    pem.apply_dem(|lat, lon| {
        // nearest-edge extension outside the DEM tile (no -40 m sea ring)
        dem.sample(lat.clamp(dsouth, dnorth), lon.clamp(dwest, deast)).map(|h| h.clamp(2.0, 250.0))
    });
    // stamp road platforms for the longest curves (roads sit 0.3 m above)
    // roads whose nodes fell outside the DEM tile got elev 0.0 — lift them
    // onto the (edge-extended) terrain so nothing sinks below the world
    let mut fixed_elev = 0usize;
    for c in imp.curves.iter_mut() {
        for p in c.points.iter_mut() {
            if p.elev.abs() < f64::EPSILON {
                let (lat, lon) = engine::terrain::local_to_latlon(p.x, p.z);
                if let Some(h) = dem
                    .sample(lat.clamp(dsouth, dnorth), lon.clamp(dwest, deast))
                    .map(|h| h.clamp(2.0, 250.0))
                {
                    p.elev = h as f64;
                    fixed_elev += 1;
                }
            }
        }
    }
    // clamp EVERY point into land range: terrarium tiles carry Indian Ocean
    // pixels (−32768 m) and the occasional garbage max (+32768 m)
    let mut clamped = 0usize;
    for c in imp.curves.iter_mut() {
        for p in c.points.iter_mut() {
            let c2 = p.elev.clamp(2.0, 250.0);
            if (c2 - p.elev).abs() > 1e-9 {
                clamped += 1;
            }
            p.elev = c2;
        }
    }
    if fixed_elev > 0 || clamped > 0 {
        println!(
            "[traffic-sim] lifted {fixed_elev} + clamped {clamped} road points to land range"
        );
    }
    let mut long: Vec<&roads::model::RoadCurve> = imp.curves.iter().collect();
    long.sort_by_key(|c| std::cmp::Reverse(c.points.len()));
    for c in long.iter().take(14) {
        let pts: Vec<(f64, f64, f64)> =
            c.points.iter().map(|p| (p.x, p.z, p.elev)).collect();
        pem.stamp_roads(&pts, c.corridor_width().min(24.0));
    }
    let baker = engine::terrain::TerrainBaker::new(pem);
    let ground_parts = baker.square_stage();

    // debug: pem height stats
    {
        let side = baker.side_m();
        let (ox, oz) = baker.pem.origin;
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        let step = side / 64.0;
        let mut sx = ox;
        while sx < ox + side {
            let mut sz = oz;
            while sz < oz + side {
                let v = baker.pem.height(sx, sz);
                mn = mn.min(v);
                mx = mx.max(v);
                sz += step;
            }
            sx += step;
        }
        let zero_cells = baker
            .pem
            .chunks
            .iter()
            .flat_map(|c| c.heights.iter())
            .filter(|&&v| v == 0)
            .count();
        let mut zero_spots: Vec<String> = Vec::new();
        for c in baker.pem.chunks.iter() {
            let zc = c.heights.iter().filter(|&&v| v == 0).count();
            if zc > 0 && zero_spots.len() < 6 {
                zero_spots.push(format!(
                    "chunk({},{}) x{} z{}",
                    c.id.0, c.id.1, zc, engine::pem::RESOLUTION
                ));
            }
        }
        println!(
            "[traffic-sim] pem ground: min {:.1} max {:.1} (side {:.0} m) zero_cells {} spots {:?}",
            mn, mx, side, zero_cells, zero_spots
        );
    }

    // scene state + junctions (godot-road-generator detection: crossings,
    // T-junctions and roundabouts — roundabouts now carry lane divisions)
    let mut state = roadkit::bridge_host::SceneState::default();
    state.curves = imp.curves;
    state.auto_join();
    let junctions = state.junctions.clone();

    // traffic toolkit (Traffic-Toolkit-DOTS port)
    let tk = toolkit::TrafficToolkit::new(
        &state.curves,
        &junctions,
        toolkit::ToolkitSettings { density: 14.0, ..Default::default() },
    );

    let mut report = Vec::new();
    report.push(format!(
        "data: {} curves · {} bridges · {} brt ways · elev {:.1}-{:.1} m",
        state.curves.len(),
        imp.stats.bridges,
        imp.stats.brt_ways,
        imp.stats.elev_min,
        imp.stats.elev_max
    ));
    for b in &bridge_info {
        report.push(format!(
            "bridge: {} ({}) {:.0} m · deck y {:.1} · clearance {:.1} m · {} piers",
            b.name, b.kind, b.length_m, b.deck_y, b.clear_below_m, b.pier_count
        ));
    }
    report.push(format!("junctions detected: {}", junctions.len()));

    let library = roadkit::library::AssetLibrary::load_dir(&assets);
    let textures = roadkit::textures::TextureLibrary::load_dir(&assets.join("textures"));
    Ok(Scene {
        state,
        library,
        textures,
        bridge_parts,
        bridge_info,
        ground_parts,
        toolkit: Some(tk),
        report,
    })
}

fn mesh_to_scene_part(m: &roads::mesh3d::MeshPart) -> roadkit::scene::ScenePart {
    roadkit::scene::ScenePart {
        positions: m.positions.clone(),
        indices: m.indices.clone(),
        color: m.color,
        uv: m.uv.clone(),
        tex: m.tex,
    }
}

/// Render one camera shot to RGBA with the yakui HUD overlay.
#[allow(clippy::too_many_arguments)]
pub fn render_frame(
    sc: &Scene,
    cam: &gdmath::OrbitCam,
    w: usize,
    h: usize,
    ss: usize,
    hud: bool,
    selected_tool: usize,
    title: &str,
    status_left: &str,
    clock: &str,
) -> Vec<u8> {
    let mut props = sc.state.props.clone();
    if let Some(tk) = &sc.toolkit {
        props.extend(tk.sim.prop_instances(&sc.state.curves));
    }
    let mut scene = roadkit::scene::build_scene_full(
        &sc.state.curves,
        &sc.state.junctions,
        &props,
        &[],
        &sc.library,
        true,
        false,
        None,
    );
    // inject ground (baked square terrain) + bridges
    for g in &sc.ground_parts {
        scene.ground.push(mesh_to_scene_part(g));
    }
    for b in &sc.bridge_parts {
        scene.parts.push(mesh_to_scene_part(b));
    }
    let big = roadkit::scene::render_scene(
        &scene,
        &sc.library,
        &sc.textures,
        cam,
        w * ss,
        h * ss,
        SKY_CLEAR,
        &[],
    );
    let mut rgba = if ss > 1 { downsample(&big, w * ss, h * ss, ss) } else { big };
    paint_sky(&mut rgba, w, h, cam.pitch);

    if hud {
        let mut buf = rgba;
        let mut p = tsui::Painter::new(&mut buf, w, h);
        let icons: Vec<(String, Vec<u8>)> = engine::tools::TOOLS
            .iter()
            .filter_map(|(name, _)| {
                let path = std::path::Path::new(ASSETS).join("ui/icons").join(format!("{name}.png"));
                std::fs::read(path).ok().map(|b| (name.to_string(), b))
            })
            .collect();
        if !icons.is_empty() {
            let props_lines = vec![
                "traffic dir R,R,F,F - auto lanes".into(),
                "gutter profile 2.0 / -0.5".into(),
                "roundabouts: lane divisions on".into(),
                "osm+mapbox: live import".into(),
            ];
            tsui::draw_toolbox(
                &mut p,
                &icons,
                selected_tool,
                engine::tools::TOOLS[selected_tool].1.name(),
                &props_lines,
                clock,
                title,
            );
            tsui::draw_status(&mut p, status_left, "yakui hud - software raster");
        }
        return buf;
    }
    rgba
}

/// Sky clear color (light haze — also fills voids beyond the terrain square).
const SKY_CLEAR: [u8; 4] = [178, 199, 220, 255];

/// Replace flat background pixels ABOVE the horizon with a real-world sky
/// gradient (zenith blue -> pale haze). Only pixels equal to the clear color
/// are touched, so geometry that reaches above the horizon (bridge decks,
/// buildings) is preserved. Horizon screen row for a roll-free OrbitCam with
/// vertical fov 0.9: y_h = h/2 · (1 − tan(pitch)/tan(fov/2)); row 0 = top.
fn paint_sky(rgba: &mut [u8], w: usize, h: usize, pitch: f32) {
    const FOV_V: f32 = 0.9;
    let t = pitch.tan() / (FOV_V / 2.0).tan();
    let y_h = ((h as f32) / 2.0 * (1.0 - t)).round();
    let y_h = (y_h as i32).clamp(0, h as i32) as usize;
    if y_h == 0 {
        return;
    }
    let zen = [92.0f32, 138.0, 198.0];
    let hor = [199.0f32, 214.0, 229.0];
    for y in 0..y_h {
        let f = (y as f32 / y_h as f32).powf(0.8);
        let r = (zen[0] + (hor[0] - zen[0]) * f) as u8;
        let g = (zen[1] + (hor[1] - zen[1]) * f) as u8;
        let b = (zen[2] + (hor[2] - zen[2]) * f) as u8;
        let row = y * w * 4;
        for x in 0..w {
            let i = row + x * 4;
            let is_bg = (rgba[i] as i32 - SKY_CLEAR[0] as i32).abs() <= 4
                && (rgba[i + 1] as i32 - SKY_CLEAR[1] as i32).abs() <= 4
                && (rgba[i + 2] as i32 - SKY_CLEAR[2] as i32).abs() <= 4;
            if is_bg {
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
            }
        }
    }
}

fn downsample(src: &[u8], sw: usize, sh: usize, f: usize) -> Vec<u8> {
    let (dw, dh) = (sw / f, sh / f);
    let mut out = vec![0u8; dw * dh * 4];
    let inv = 1.0 / (f * f) as f32;
    for dy in 0..dh {
        for dx in 0..dw {
            let (mut r, mut g, mut b, mut a) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
            for sy in 0..f {
                let row = (dy * f + sy) * sw;
                for sx in 0..f {
                    let i = (row + dx * f + sx) * 4;
                    r += src[i] as f32;
                    g += src[i + 1] as f32;
                    b += src[i + 2] as f32;
                    a += src[i + 3] as f32;
                }
            }
            let o = (dy * dw + dx) * 4;
            out[o] = (r * inv) as u8;
            out[o + 1] = (g * inv) as u8;
            out[o + 2] = (b * inv) as u8;
            out[o + 3] = (a * inv) as u8;
        }
    }
    out
}

fn save_png(path: &std::path::Path, rgba: &[u8], w: usize, h: usize) -> Result<(), String> {
    let img = RgbaImage::from_raw(w as u32, h as u32, rgba.to_vec())
        .ok_or("bad rgba buffer")?;
    img.save(path).map_err(|e| format!("save {}: {e}", path.display()))
}

/// Run the full capture: traffic sim + camera shots + HUD + PNGs.
pub fn run_capture(cfg: &RunConfig) -> Result<Vec<String>, String> {
    let mut sc = build_data_scene(&cfg.site)?;
    let mut outs = Vec::new();
    for line in &sc.report {
        println!("[traffic-sim] {line}");
    }

    // run traffic (toolkit)
    if let Some(tk) = sc.toolkit.as_mut() {
        let steps = (cfg.sim_seconds / 0.1).ceil() as usize;
        for _ in 0..steps {
            tk.step(0.1, &sc.state.curves);
        }
        println!("[traffic-sim] {}", tk.report());
    }

    // camera shots: hero orbit + bridge detail (zoomed, real-world pass)
    let mut shots = toolkit::hero_shots(&sc.state.curves, cfg.shots, &cfg.out_prefix);
    if let Some(b) = sc
        .bridge_info
        .iter()
        .max_by(|a, c| a.length_m.partial_cmp(&c.length_m).unwrap())
    {
        // sample the LONGEST bridge's own parts only (piers/railings of other
        // bridges would drag the centroid off the span). y comes from the
        // exact deck_y field — part vertices include piers/underside.
        let mut pts: Vec<[f64; 3]> = Vec::new();
        for p in &b.parts {
            for v in p.positions.iter().step_by(5) {
                pts.push([v.x as f64, v.y as f64, v.z as f64]);
            }
        }
        if !pts.is_empty() {
            let n = pts.len() as f64;
            let cx = pts.iter().map(|p| p[0]).sum::<f64>() / n;
            let cz = pts.iter().map(|p| p[2]).sum::<f64>() / n;
            // along-deck axis: farthest sampled vertex pair (robust for
            // diagonal spans) — O(n²) on a thinned sample
            let thin: Vec<[f64; 3]> = pts.iter().step_by(4).copied().collect();
            let mut best = (0.0f64, [1.0f64, 0.0]);
            for i in 0..thin.len() {
                for j in (i + 1)..thin.len() {
                    let dx = thin[j][0] - thin[i][0];
                    let dz = thin[j][2] - thin[i][2];
                    let d2 = dx * dx + dz * dz;
                    if d2 > best.0 {
                        best = (d2, [dx, dz]);
                    }
                }
            }
            let dl = best.0.sqrt().max(1e-6);
            let axis = [best.1[0] / dl, best.1[1] / dl];
            shots.extend(toolkit::focus_shots(
                &sc.state.curves,
                [cx, b.deck_y, cz],
                b.deck_y - b.clear_below_m,
                axis,
                &format!("{}_bridge", cfg.out_prefix),
            ));
        }
    }

    let dir = std::path::Path::new(OUT_DIR);
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for (i, shot) in shots.iter().enumerate() {
        let cam = shot.to_cam();
        let rgba = render_frame(
            &sc,
            &cam,
            cfg.size,
            (cfg.size as f32 * 0.625) as usize,
            cfg.ss,
            cfg.hud,
            i % engine::tools::TOOLS.len(),
            "TRAFFIC-SIM",
            &format!("{} · {} roads", cfg.site, sc.state.curves.len()),
            "08:30",
        );
        let out = dir.join(format!("{}.png", shot.name));
        save_png(&out, &rgba, cfg.size, (cfg.size as f32 * 0.625) as usize)?;
        outs.push(out.display().to_string());
        println!("[traffic-sim] saved {}", out.display());
    }
    Ok(outs)
}
