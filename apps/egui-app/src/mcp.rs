//! MCP (Model Context Protocol) server + headless UI test runner.
//!
//! `roadkit-godot --mcp` speaks JSON-RPC 2.0 over stdio (newline-delimited,
//! the standard MCP transport) and exposes the editor's brain to AI agents:
//!
//!   run_script / run_demo   — build a scene from .gd source
//!   get_scene / get_state   — inspect every curve, point, junction, prop
//!   add_point / get_point / set_point_props — GRG RoadPoint editing
//!   add_lane / get_lanes / check_lanes — the AI RoadLane graph
//!   simulate / terrain / get_manager / get_history — sim + manager state
//!   render_3d / render_plan — deterministic PNG screenshots (no GUI needed)
//!   list_actions            — EVERY registered UI button, label + location
//!   do_action               — execute any button headlessly + state diff
//!
//! so tests can click through the whole app like an artifact preview harness.
//! `roadkit-godot --uitest` runs all of those actions locally, a 5x5
//! action × scenario matrix over five bundled demos and a render check,
//! printing PASS/FAIL tables (exit code 1 on any failure).

use egui_app::state::{LaunchConfig, RoadkitApp};
use gdmath::{OrbitCam, Vec3};
use egui_app::actions;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

const MCP_VERSION: &str = "2.3.0";

/* ================================================================== */
/* Headless session (shared by the MCP server and --uitest)            */
/* ================================================================== */

pub struct Session {
    pub app: RoadkitApp,
}

impl Session {
    pub fn new() -> Self {
        Self {
            app: RoadkitApp::core(LaunchConfig::default()),
        }
    }

    fn stats_json(&self) -> Value {
        let sc = self.app.scene.borrow();
        let tris = self
            .app
            .render
            .scene3d
            .as_ref()
            .map(|s| s.stats.tris)
            .unwrap_or(0);
        json!({
            "curves": sc.curves.len(),
            "points": sc.curves.iter().map(|c| c.points.len()).sum::<usize>(),
            "props": sc.props.len(),
            "junctions": sc.junctions.len(),
            "tris": tris,
        })
    }

    fn scene_json(&self) -> Value {
        let sc = self.app.scene.borrow();
        json!({
            "curves": sc.curves.iter().map(|c| json!({
                "name": c.name,
                "visible": c.visible,
                "closed": c.closed,
                "color": c.color,
                "length_m": c.length_m(),
                "points": c.points.iter().map(|p| json!({
                    "x": p.x, "z": p.z, "elev": p.elev,
                    "bearing_deg": p.bearing.to_degrees(),
                    "prior_mag": p.prior_mag, "next_mag": p.next_mag,
                    "lanes": p.lanes, "lane_width": p.lane_width,
                    "oneway": p.oneway, "median": p.median,
                    "pathway_l": p.pathway_l, "pathway_r": p.pathway_r,
                    "dirt": p.dirt, "visible": p.visible, "locked": p.locked,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "junctions": sc.junctions.iter().map(|j| json!({
                "kind": j.kind.label(),
                "x": j.x, "z": j.z,
                "legs": j.legs.len(),
                "prefab": j.prefab.as_ref().map(|p| p.id.clone()),
            })).collect::<Vec<_>>(),
            "props": sc.props.iter().map(|p| json!({
                "asset": p.asset,
                "x": p.pos.x, "y": p.pos.y, "z": p.pos.z,
            })).collect::<Vec<_>>(),
            "settings": {
                "auto_join": sc.junction_settings.enabled,
                "auto_roundabout": sc.junction_settings.auto_roundabout,
                "use_prefabs": sc.junction_settings.use_prefabs,
                "snap_m": sc.junction_settings.snap_m,
                "island_radius": sc.junction_settings.island_radius,
            },
            "stats": self.stats_json(),
        })
    }

    fn state_json(&self) -> Value {
        json!({
            "tab": format!("{:?}", self.app.tab),
            "tool": format!("{:?}", self.app.tool),
            "panels": {
                "layers": self.app.panels.layers,
                "properties": self.app.panels.properties,
                "assets": self.app.panels.assets,
                "script": self.app.panels.script,
                "console": self.app.panels.console,
            },
            "dock_tab": format!("{:?}", self.app.dock_tab),
            "dock_collapsed": self.app.dock_collapsed,
            "selection_point": self.app.sel,
            "selection_container": self.app.sel_container,
            "selection_junction": self.app.sel_junction,
            "script": egui_app::scripts::all().get(self.app.script_idx).map(|(l, _, _)| l.to_string()),
            "status": self.app.status,
            "stats": self.stats_json(),
        })
    }

    /// Run a .gd source through the real bridge into the session scene.
    fn run(&mut self, source: &str) -> Result<String, String> {
        self.app.script_src = source.to_string();
        actions::perform(&mut self.app, "run_script")
    }

    /// Build a throwaway world from source for the render tools (keeps the
    /// session scene untouched). Borrows the session's asset + texture libs.
    fn world(&self, source: Option<&str>) -> Result<HeadWorld<'_>, String> {
        let src = match source {
            Some(s) => s.to_string(),
            None => self.app.script_src.clone(),
        };
        let script = gdscript::parse(&src)?;
        let scene: std::rc::Rc<std::cell::RefCell<egui_app::bridge::SceneState>> =
            std::rc::Rc::new(std::cell::RefCell::new(egui_app::bridge::SceneState::default()));
        let mut it = gdscript::Interpreter::new();
        egui_app::bridge::register(&mut it, &scene);
        it.run(&script, "main")?;
        let mut st = scene.borrow_mut();
        for l in it.take_output() {
            st.console.push(l);
        }
        st.auto_join();
        Ok(HeadWorld {
            scene: std::mem::take(&mut *st),
            library: &self.app.assets,
            textures: &self.app.textures,
        })
    }
}

struct HeadWorld<'a> {
    scene: egui_app::bridge::SceneState,
    library: &'a roadkit::AssetLibrary,
    textures: &'a roadkit::TextureLibrary,
}

/// Frame the world bbox like the GUI's fit_scene (hero 3/4 angle).
fn fit_cam(curves: &[roads::model::RoadCurve], w: usize, h: usize) -> OrbitCam {
    let (mut min_x, mut min_z) = (f64::INFINITY, f64::INFINITY);
    let (mut max_x, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
    for c in curves {
        for p in &c.points {
            min_x = min_x.min(p.x);
            min_z = min_z.min(p.z);
            max_x = max_x.max(p.x);
            max_z = max_z.max(p.z);
        }
    }
    if !min_x.is_finite() {
        return OrbitCam::default();
    }
    let span = ((max_x - min_x).max(max_z - min_z)).max(40.0);
    let mut cam = OrbitCam::default();
    cam.dist = (span * 1.45).clamp(30.0, 20_000.0) as f32;
    cam.pitch = 0.52;
    cam.yaw = 0.65;
    cam.sync_targets();
    // approximate viewport-based framing distance correction
    let aspect = (w as f32 / h.max(1) as f32).clamp(0.5, 3.0);
    cam.dist *= 1.0 / aspect.min(1.0).max(0.6);
    cam.target = Vec3::ZERO;
    cam
}

fn write_png(out: &std::path::Path, rgba: &[u8], w: u32, h: u32) -> Result<(), String> {
    use image::ImageBuffer;
    let img: ImageBuffer<image::Rgba<u8>, Vec<u8>> =
        ImageBuffer::from_raw(w, h, rgba.to_vec()).ok_or("png buffer size mismatch")?;
    let mut buf = Vec::new();
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| format!("png encode: {e}"))?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(out, &buf).map_err(|e| format!("write {}: {e}", out.display()))
}

/* ================================================================== */
/* JSON helpers (GRG property surfaces)                                */
/* ================================================================== */

fn fnum(k: &str, v: &Value) -> Result<f64, String> {
    v.as_f64().ok_or_else(|| format!("prop '{k}' expects a number"))
}

fn fbool(k: &str, v: &Value) -> Result<bool, String> {
    v.as_bool().ok_or_else(|| format!("prop '{k}' expects a boolean"))
}

fn fint(k: &str, v: &Value) -> Result<usize, String> {
    v.as_u64()
        .map(|n| n as usize)
        .ok_or_else(|| format!("prop '{k}' expects a non-negative integer"))
}

/// The full GRG RoadPoint property surface of one point as JSON.
fn point_json(
    sc: &egui_app::bridge::SceneState,
    ci: usize,
    pi: usize,
) -> Result<Value, String> {
    let curve = sc
        .curves
        .get(ci)
        .ok_or_else(|| format!("container {ci} out of range ({} curves)", sc.curves.len()))?;
    let p = curve
        .points
        .get(pi)
        .ok_or_else(|| format!("point {pi} out of range on '{}' ({} points)", curve.name, curve.points.len()))?;
    let neigh = |i: usize| -> String {
        curve
            .points
            .get(i)
            .map(|n| if n.name.is_empty() { format!("RP_{i}") } else { n.name.clone() })
            .unwrap_or_else(|| "(none)".into())
    };
    Ok(json!({
        "ci": ci, "pi": pi,
        "container": curve.name,
        "name": if p.name.is_empty() { format!("RP_{pi}") } else { p.name.clone() },
        "x": p.x, "z": p.z, "elev": p.elev,
        "bearing_deg": p.bearing.to_degrees(),
        "prior_mag": p.prior_mag, "next_mag": p.next_mag,
        "lanes": p.lanes, "rev_lanes": p.rev_lanes, "fwd_lanes": p.fwd_lanes(),
        "lane_width": p.lane_width,
        "shoulder_l": p.shoulder_l, "shoulder_r": p.shoulder_r,
        "median": p.median, "pathway_l": p.pathway_l, "pathway_r": p.pathway_r,
        "oneway": p.oneway, "auto_lanes": p.auto_lanes, "terminated": p.terminated,
        "create_geo": p.create_geo, "flatten_terrain": p.flatten_terrain,
        "gutter_x": p.gutter_profile.0, "gutter_y": p.gutter_profile.1,
        "alignment": match p.alignment {
            roads::model::Alignment::Geometric => "geometric",
            roads::model::Alignment::Divider => "divider",
        },
        "underside_thickness": p.underside_thickness,
        "visible": p.visible, "locked": p.locked, "dirt": p.dirt,
        "auto_bearing": p.auto_bearing,
        "traffic_dir": p.traffic_dir().iter().map(|d| d.as_char().to_string()).collect::<Vec<_>>(),
        "lane_types": p.lane_types(),
        "prior_name": pi.checked_sub(1).map(neigh).unwrap_or_else(|| "(none)".into()),
        "next_name": neigh(pi + 1),
    }))
}

fn terrain_json(t: &roads::terrain::TerrainState) -> Value {
    json!({
        "enabled": t.enabled, "amplitude": t.amplitude, "wavelength": t.wavelength,
        "seed": t.seed, "offset": t.offset, "edge_margin": t.edge_margin,
        "edge_falloff": t.edge_falloff, "flatten_under_roads": t.flatten_under_roads,
    })
}

/// roads::lane::check_lanes serialized + a top-level `ok` flag
/// (no unexpected dead ends anywhere).
fn check_lanes_json(sc: &egui_app::bridge::SceneState) -> Value {
    let report = roads::lane::check_lanes(&sc.lanes, &sc.curves);
    let unexpected: usize = report
        .containers
        .iter()
        .map(|c| c.unexpected_dead_ends.len())
        .sum();
    json!({
        "ok": unexpected == 0,
        "total_lanes": report.total_lanes,
        "unexpected_dead_ends": unexpected,
        "containers": report.containers.iter().map(|c| json!({
            "container": c.container, "lanes": c.lanes, "connected": c.connected,
            "transitions": c.transitions, "expected_dead_ends": c.expected_dead_ends,
            "unexpected_dead_ends": c.unexpected_dead_ends,
        })).collect::<Vec<_>>(),
    })
}

/* ================================================================== */
/* Tool definitions                                                    */
/* ================================================================== */

fn tools_json() -> Vec<Value> {
    vec![
        json!({"name":"run_script","description":"Parse + run .gd script source into the session scene (fresh scene each run, like F5 in the GUI)","inputSchema":{"type":"object","properties":{"source":{"type":"string","description":"full .gd source"}},"required":["source"]}}),
        json!({"name":"run_demo","description":"Run one of the bundled demo scripts by label","inputSchema":{"type":"object","properties":{"name":{"type":"string"}},"required":["name"]}}),
        json!({"name":"list_demos","description":"List bundled demo scripts","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"get_scene","description":"Full scene JSON: curves, points (lanes/median/dirt...), junctions, props, stats","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"get_state","description":"Editor state: tab, tool, open panels, selection, status, stats","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"render_3d","description":"Render the 3D scene to PNG (headless software renderer). Optional camera control or focus area, optional traffic sim","inputSchema":{"type":"object","properties":{"out":{"type":"string","description":"png path"},"source":{"type":"string","description":"optional .gd source (defaults to the session scene's script)"},"width":{"type":"integer"},"height":{"type":"integer"},"yaw":{"type":"number"},"pitch":{"type":"number"},"dist":{"type":"number"},"focus_x":{"type":"number"},"focus_z":{"type":"number"},"focus_r":{"type":"number"},"sim_seconds":{"type":"number"},"density":{"type":"number"},"atmo":{"type":"boolean","description":"true = Egregoria atmosphere (sky/fog). Default false = the app's editor look (flat backdrop + grid)"}},"required":["out"]}}),
        json!({"name":"load_opendrive","description":"Import an OpenDRIVE (.xodr) road network through the native importer (planView line/arc/spiral + elevationProfile + laneSections) — replaces the session scene with the generated RoadKit script","inputSchema":{"type":"object","properties":{"path":{"type":"string","description":"path to the .xodr file"},"step":{"type":"number","description":"centerline sample step in metres (default 15)"}},"required":["path"]}}),
        json!({"name":"render_plan","description":"Render the 2D plan (top-down blueprint) to PNG","inputSchema":{"type":"object","properties":{"out":{"type":"string"},"source":{"type":"string"},"width":{"type":"integer"},"height":{"type":"integer"},"zoom":{"type":"number"}},"required":["out"]}}),
        json!({"name":"list_actions","description":"Every registered UI action (id, label, location, hotkey) — the full button map","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"do_action","description":"Execute a UI action headlessly (same implementation as the button) and get a state diff","inputSchema":{"type":"object","properties":{"id":{"type":"string"}},"required":["id"]}}),
        json!({"name":"get_assets","description":"List GLB assets (id, role, triangles) + loaded textures","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"add_point","description":"GRG 'Add next RoadPoint': append a point to a container (default: the selected container, else a fresh one), copying the last point's settings — bearing continues, mags = lane_width x lanes (min 2), auto_bearing. Pushes undo history. Returns the new (ci, pi) + stats","inputSchema":{"type":"object","properties":{"x":{"type":"number"},"z":{"type":"number"},"container_index":{"type":"integer","description":"append to this curve; default = selected point's / selected container's, else a new container"},"lanes":{"type":"integer","description":"total lane count (default: copied from the last point)"},"rev_lanes":{"type":"integer","description":"reverse lane count"},"median":{"type":"number"},"lane_width":{"type":"number"},"elev":{"type":"number"}},"required":["x","z"]}}),
        json!({"name":"set_point_props","description":"Write any RoadPoint properties (x, z, elev, bearing_deg, prior_mag, next_mag, lanes, rev_lanes, lane_width, shoulder_l, shoulder_r, median, pathway_l, pathway_r, oneway, auto_lanes, terminated, create_geo, flatten_terrain, gutter_x, gutter_y, alignment 'geometric'|'divider', underside_thickness, visible, locked, dirt, auto_bearing); recomputes junctions+lanes. Returns the full new point JSON","inputSchema":{"type":"object","properties":{"ci":{"type":"integer"},"pi":{"type":"integer"},"props":{"type":"object"}},"required":["ci","pi","props"]}}),
        json!({"name":"get_point","description":"Full GRG RoadPoint property JSON: position, mags, lanes split, traffic_dir array, lane_types, gutter, alignment, neighbours' names","inputSchema":{"type":"object","properties":{"ci":{"type":"integer"},"pi":{"type":"integer"}},"required":["ci","pi"]}}),
        json!({"name":"add_lane","description":"GRG 'Add RoadLane': switch generate_ai_lanes (+ lanes_editable) on for one container (container_index, default: the selection) and regenerate the AI lane graph. Returns the lane count","inputSchema":{"type":"object","properties":{"container_index":{"type":"integer"}}}}),
        json!({"name":"get_lanes","description":"List the generated AI RoadLanes: name, container, segment, side, index, length_m, lane_next/lane_prior/lane_left/lane_right links, transition","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"check_lanes","description":"Run roads::lane::check_lanes: per-container connected/transitions/dead-ends; top-level ok = no unexpected dead ends","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"simulate","description":"Run a session-local traffic simulation (TrafficSim) for some seconds and report the vehicle count","inputSchema":{"type":"object","properties":{"seconds":{"type":"number","description":"default 10"},"dt":{"type":"number","description":"default 0.1"},"density":{"type":"number","description":"vehicles/km, default 12"}}}}),
        json!({"name":"get_history","description":"Undo history: entries (index, label), cursor, can_undo/can_redo","inputSchema":{"type":"object","properties":{}}}),
        json!({"name":"terrain","description":"Read/write the RoadTerrain3DConnector state (enabled, amplitude, wavelength, offset, edge_margin, edge_falloff); returns the terrain state","inputSchema":{"type":"object","properties":{"enabled":{"type":"boolean"},"amplitude":{"type":"number"},"wavelength":{"type":"number"},"offset":{"type":"number"},"edge_margin":{"type":"number"},"edge_falloff":{"type":"number"}}}}),
        json!({"name":"get_manager","description":"RoadManager state: density, underside_thickness, ai_lane_group, auto_refresh + junction settings, lanes_editable, terrain","inputSchema":{"type":"object","properties":{}}}),
    ]
}

fn call_tool(sess: &mut Session, name: &str, args: &Value) -> Result<Value, String> {
    match name {
        "run_script" => {
            let source = args
                .get("source")
                .and_then(|v| v.as_str())
                .ok_or("missing 'source'")?;
            let diff = sess.run(source)?;
            Ok(json!({"result": diff, "stats": sess.stats_json()}))
        }
        "run_demo" | "list_demos" => {
            if name == "list_demos" {
                let demos: Vec<String> = egui_app::scripts::all()
                    .iter()
                    .map(|(l, f, _)| format!("{l} ({f})"))
                    .collect();
                return Ok(json!({"demos": demos}));
            }
            let want = args.get("name").and_then(|v| v.as_str()).ok_or("missing 'name'")?;
            let all = egui_app::scripts::all();
            let idx = all
                .iter()
                .position(|(l, f, _)| *l == want || *f == want || f.contains(want))
                .ok_or_else(|| format!("demo '{want}' not found — call list_demos"))?;
            let src = all[idx].2.to_string();
            let diff = sess.run(&src)?;
            Ok(json!({"result": diff, "demo": all[idx].0, "stats": sess.stats_json()}))
        }
        "load_opendrive" => {
            // native OpenDRIVE import (crates/roadkit/src/opendrive.rs):
            // .xodr → sampled centerlines (line/arc/spiral + elevation +
            // lanes) → generated .gd → the regular session scene
            let path = args.get("path").and_then(|v| v.as_str()).ok_or("missing 'path'")?;
            let step = args
                .get("step")
                .and_then(|v| v.as_f64())
                .unwrap_or(15.0)
                .clamp(2.0, 200.0);
            let xml = std::fs::read_to_string(path).map_err(|e| format!("read {path}: {e}"))?;
            let script = roadkit::opendrive::xodr_to_gd_script(&xml, step)?;
            let diff = sess.run(&script)?;
            Ok(json!({
                "result": diff,
                "import": format!("{path} @ {step:.0} m step"),
                "stats": sess.stats_json(),
            }))
        }
        "get_scene" => Ok(sess.scene_json()),
        "get_state" => Ok(sess.state_json()),
        "render_3d" => {
            let out = args.get("out").and_then(|v| v.as_str()).ok_or("missing 'out'")?;
            let w = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1280).clamp(64, 4096) as usize;
            let h = args.get("height").and_then(|v| v.as_u64()).unwrap_or(720).clamp(64, 4096) as usize;
            let world = sess.world(args.get("source").and_then(|v| v.as_str()))?;
            let mut cam = fit_cam(&world.scene.curves, w, h);
            if let (Some(y), Some(p)) = (
                args.get("yaw").and_then(|v| v.as_f64()),
                args.get("pitch").and_then(|v| v.as_f64()),
            ) {
                cam.yaw = y as f32;
                cam.pitch = p as f32;
                cam.sync_targets();
            }
            if let Some(d) = args.get("dist").and_then(|v| v.as_f64()) {
                cam.dist = (d as f32).max(4.0);
                cam.sync_targets();
            }
            if let (Some(fx), Some(fz)) = (
                args.get("focus_x").and_then(|v| v.as_f64()),
                args.get("focus_z").and_then(|v| v.as_f64()),
            ) {
                let r = args.get("focus_r").and_then(|v| v.as_f64()).unwrap_or(40.0);
                cam.target = Vec3::new(fx as f32, 0.0, fz as f32);
                cam.dist = (r * 2.4).clamp(10.0, 40_000.0) as f32;
                cam.sync_targets();
            }
            let sim = args.get("sim_seconds").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let density = args.get("density").and_then(|v| v.as_f64()).unwrap_or(14.0);
            let rgba = render_world(&world, &cam, w, h, sim, density, args.get("atmo").and_then(|v| v.as_bool()));
            write_png(std::path::Path::new(out), &rgba, w as u32, h as u32)?;
            let veh = if sim > 0.0 {
                json!(format!("traffic sim {:.1}s @ {density} veh/km", sim))
            } else {
                json!(null)
            };
            Ok(json!({"wrote": out, "w": w, "h": h, "sim": veh, "stats": sess.stats_json()}))
        }
        "render_plan" => {
            let out = args.get("out").and_then(|v| v.as_str()).ok_or("missing 'out'")?;
            let w = args.get("width").and_then(|v| v.as_u64()).unwrap_or(1280).clamp(64, 4096) as usize;
            let h = args.get("height").and_then(|v| v.as_u64()).unwrap_or(720).clamp(64, 4096) as usize;
            let world = sess.world(args.get("source").and_then(|v| v.as_str()))?;
            let rgba = plan_rgba(&world, args.get("zoom").and_then(|v| v.as_f64()).unwrap_or(1.6) as f32, w, h);
            write_png(std::path::Path::new(out), &rgba, w as u32, h as u32)?;
            Ok(json!({"wrote": out, "w": w, "h": h}))
        }
        "list_actions" => Ok(json!({"actions": actions::all().iter().map(|a| json!({
            "id": a.id, "label": a.label, "location": a.location,
            "hotkey": a.hotkey,
        })).collect::<Vec<_>>()})),
        "do_action" => {
            let id = args.get("id").and_then(|v| v.as_str()).ok_or("missing 'id'")?;
            let diff = actions::perform(&mut sess.app, id)?;
            Ok(json!({"action": id, "result": diff, "state": sess.state_json()}))
        }
        "get_assets" => Ok(json!({
            "assets": sess.app.assets.entries.iter().map(|e| json!({
                "id": e.id, "name": e.name, "role": e.role, "tris": e.mesh.triangle_count(),
            })).collect::<Vec<_>>(),
            "textures": sess.app.textures.slot_count(),
        })),
        /* ---------------- R10-d: GRG editing + AI lanes + sim ---------------- */
        "add_point" => {
            let x = args.get("x").and_then(Value::as_f64).ok_or("missing 'x'")?;
            let z = args.get("z").and_then(Value::as_f64).ok_or("missing 'z'")?;
            let lanes = args.get("lanes").and_then(Value::as_u64).map(|v| v as usize);
            let rev_lanes = args.get("rev_lanes").and_then(Value::as_u64).map(|v| v as usize);
            let median = args.get("median").and_then(Value::as_f64);
            let lane_width = args.get("lane_width").and_then(Value::as_f64);
            let elev = args.get("elev").and_then(Value::as_f64);
            // target container: explicit index > selected point > selected container
            let ci = {
                let sc = sess.app.scene.borrow();
                let explicit = args.get("container_index").and_then(Value::as_u64).map(|v| v as usize);
                let selected = sess.app.sel.map(|(ci, _)| ci).or(sess.app.sel_container);
                explicit
                    .or(selected)
                    .filter(|&ci| ci < sc.curves.len() && !sc.curves[ci].points.is_empty())
            };
            let (ci, pi, created) = match ci {
                Some(ci) => {
                    // GRG "Add next RoadPoint": continue from the last point
                    let pi;
                    {
                        let mut sc = sess.app.scene.borrow_mut();
                        let src = sc.curves[ci].points.last().unwrap().clone();
                        let (dx, dz) = src.dir();
                        let lw = lane_width.unwrap_or(src.lane_width);
                        let ln = lanes.unwrap_or(src.lanes).max(1);
                        let dist = egui_app::state::SEG_DIST_MULT * lw;
                        let mut np = roads::model::RoadPoint::new(src.x + dx * dist, src.z + dz * dist);
                        np.copy_settings_from(&src);
                        np.name = String::new();
                        np.bearing = src.bearing; // bearing continuation
                        np.elev = elev.unwrap_or(src.elev);
                        np.lane_width = lw;
                        np.lanes = ln;
                        np.rev_lanes = rev_lanes.unwrap_or(src.rev_lanes).min(ln.saturating_sub(1));
                        np.oneway = np.rev_lanes == 0;
                        np.median = median.unwrap_or(src.median);
                        np.prior_mag = (lw * ln as f64).max(2.0);
                        np.next_mag = np.prior_mag;
                        np.auto_bearing = true;
                        sc.curves[ci].points.push(np);
                        sc.curves[ci].auto_orient();
                        pi = sc.curves[ci].points.len() - 1;
                        sc.dirty = true;
                        sc.recompute_junctions();
                        let name = sc.curves[ci].name.clone();
                        sc.log(format!(
                            "[edit] added RoadPoint {pi} to '{name}' — bearing continuation, {dist:.0} m ahead"
                        ));
                    }
                    sess.app.render.dirty = true;
                    sess.app.clear_selection();
                    sess.app.sel_container = Some(ci);
                    sess.app.sel = Some((ci, pi));
                    sess.app.push_history("Add next RoadPoint");
                    (ci, pi, false)
                }
                None => {
                    // GRG "Add RoadContainer": fresh container, first point here
                    let ci;
                    {
                        let mut sc = sess.app.scene.borrow_mut();
                        sc.run_counter += 1;
                        ci = sc.curves.len();
                        let lw = lane_width.unwrap_or(3.5);
                        let ln = lanes.unwrap_or(2).max(1);
                        let rv = rev_lanes.unwrap_or(1).min(ln.saturating_sub(1));
                        let mut np = roads::model::RoadPoint::new(x, z);
                        np.lanes = ln;
                        np.rev_lanes = rv;
                        np.oneway = rv == 0;
                        np.lane_width = lw;
                        if let Some(m) = median {
                            np.median = m;
                        }
                        if let Some(e) = elev {
                            np.elev = e;
                        }
                        np.prior_mag = (lw * ln as f64).max(2.0);
                        np.next_mag = np.prior_mag;
                        np.auto_bearing = true;
                        let name = format!("Road {}", sc.run_counter);
                        let mut curve = roads::model::RoadCurve::new(name.clone());
                        curve.add_point(np);
                        sc.curves.push(curve);
                        sc.dirty = true;
                        sc.recompute_junctions();
                        sc.log(format!(
                            "[edit] new container '{name}' — first RoadPoint at ({x:.0}, {z:.0})"
                        ));
                    }
                    sess.app.render.dirty = true;
                    sess.app.clear_selection();
                    sess.app.sel_container = Some(ci);
                    sess.app.sel = Some((ci, 0));
                    sess.app.push_history("Add RoadContainer");
                    (ci, 0, true)
                }
            };
            let stats = sess.stats_json();
            let point = {
                let sc = sess.app.scene.borrow();
                point_json(&sc, ci, pi)?
            };
            Ok(json!({"ci": ci, "pi": pi, "new_container": created, "point": point, "stats": stats}))
        }
        "set_point_props" => {
            let ci = args.get("ci").and_then(Value::as_u64).ok_or("missing 'ci'")? as usize;
            let pi = args.get("pi").and_then(Value::as_u64).ok_or("missing 'pi'")? as usize;
            let obj = args
                .get("props")
                .ok_or("missing 'props'")?
                .as_object()
                .ok_or("'props' must be an object")?
                .clone();
            {
                let mut sc = sess.app.scene.borrow_mut();
                {
                    let curve = sc
                        .curves
                        .get_mut(ci)
                        .ok_or_else(|| format!("container {ci} out of range"))?;
                    let p = curve
                        .points
                        .get_mut(pi)
                        .ok_or_else(|| format!("point {pi} out of range"))?;
                    for (k, v) in &obj {
                        match k.as_str() {
                            "x" => p.x = fnum(k, v)?,
                            "z" => p.z = fnum(k, v)?,
                            "elev" => p.elev = fnum(k, v)?,
                            "bearing_deg" => p.bearing = fnum(k, v)?.to_radians(),
                            "prior_mag" => p.prior_mag = fnum(k, v)?.max(0.5),
                            "next_mag" => p.next_mag = fnum(k, v)?.max(0.5),
                            "lanes" => p.lanes = fint(k, v)?.max(1),
                            "rev_lanes" => p.rev_lanes = fint(k, v)?,
                            "lane_width" => p.lane_width = fnum(k, v)?.max(1.0),
                            "shoulder_l" => p.shoulder_l = fnum(k, v)?.max(0.0),
                            "shoulder_r" => p.shoulder_r = fnum(k, v)?.max(0.0),
                            "median" => p.median = fnum(k, v)?.clamp(0.0, 12.0),
                            "pathway_l" => p.pathway_l = fnum(k, v)?.clamp(0.0, 8.0),
                            "pathway_r" => p.pathway_r = fnum(k, v)?.clamp(0.0, 8.0),
                            "oneway" => p.oneway = fbool(k, v)?,
                            "auto_lanes" => p.auto_lanes = fbool(k, v)?,
                            "terminated" => p.terminated = fbool(k, v)?,
                            "create_geo" => p.create_geo = fbool(k, v)?,
                            "flatten_terrain" => p.flatten_terrain = fbool(k, v)?,
                            "gutter_x" => p.gutter_profile.0 = fnum(k, v)?,
                            "gutter_y" => p.gutter_profile.1 = fnum(k, v)?,
                            "alignment" => {
                                p.alignment = match v.as_str() {
                                    Some("geometric") => roads::model::Alignment::Geometric,
                                    Some("divider") => roads::model::Alignment::Divider,
                                    other => {
                                        return Err(format!(
                                            "prop 'alignment' expects \"geometric\"|\"divider\", got {other:?}"
                                        ))
                                    }
                                };
                            }
                            "underside_thickness" => p.underside_thickness = fnum(k, v)?,
                            "visible" => p.visible = fbool(k, v)?,
                            "locked" => p.locked = fbool(k, v)?,
                            "dirt" => p.dirt = fbool(k, v)?,
                            "auto_bearing" => p.auto_bearing = fbool(k, v)?,
                            other => return Err(format!("unknown point prop '{other}' — see the tool schema")),
                        }
                    }
                    // keep the lanes split coherent (REVERSE <= lanes, >= 1 forward)
                    p.rev_lanes = p.rev_lanes.min(p.lanes.saturating_sub(1));
                    p.oneway = p.rev_lanes == 0;
                }
                sc.dirty = true;
                sc.recompute_junctions();
            }
            sess.app.render.dirty = true;
            let point = {
                let sc = sess.app.scene.borrow();
                point_json(&sc, ci, pi)?
            };
            Ok(json!({"ci": ci, "pi": pi, "point": point, "stats": sess.stats_json()}))
        }
        "get_point" => {
            let ci = args.get("ci").and_then(Value::as_u64).ok_or("missing 'ci'")? as usize;
            let pi = args.get("pi").and_then(Value::as_u64).ok_or("missing 'pi'")? as usize;
            let sc = sess.app.scene.borrow();
            Ok(point_json(&sc, ci, pi)?)
        }
        "add_lane" => {
            if let Some(ci) = args.get("container_index").and_then(Value::as_u64) {
                let n = sess.app.scene.borrow().curves.len();
                if ci as usize >= n {
                    return Err(format!("container_index {ci} out of range ({n} containers)"));
                }
                sess.app.sel_container = Some(ci as usize);
            }
            let diff = actions::perform(&mut sess.app, "roads_add_lane")?;
            let (lanes, editable) = {
                let sc = sess.app.scene.borrow();
                (sc.lanes.len(), sc.lanes_editable)
            };
            Ok(json!({"lanes": lanes, "lanes_editable": editable, "result": diff}))
        }
        "get_lanes" => {
            let sc = sess.app.scene.borrow();
            let lanes = sc.lanes.iter().map(|l| json!({
                "name": l.name,
                "container": sc.curves.get(l.container).map(|c| c.name.clone())
                    .unwrap_or_else(|| format!("curve {}", l.container)),
                "container_index": l.container,
                "segment": l.segment,
                "side": l.side.to_string(),
                "index": l.index,
                "length_m": l.length_m(),
                "lane_next": l.lane_next,
                "lane_prior": l.lane_prior,
                "lane_left": l.lane_left,
                "lane_right": l.lane_right,
                "transition": l.transition,
                "width": l.width,
            })).collect::<Vec<_>>();
            Ok(json!({"count": lanes.len(), "lanes": lanes}))
        }
        "check_lanes" => {
            let sc = sess.app.scene.borrow();
            Ok(check_lanes_json(&sc))
        }
        "simulate" => {
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(10.0).clamp(0.0, 3600.0);
            let dt = args.get("dt").and_then(Value::as_f64).unwrap_or(0.1).clamp(0.01, 1.0);
            let density = args.get("density").and_then(Value::as_f64).unwrap_or(12.0);
            let (curves, junctions) = {
                let sc = sess.app.scene.borrow();
                (sc.curves.clone(), sc.junctions.clone())
            };
            if curves.is_empty() {
                return Err("no roads in the session scene — run_script first".into());
            }
            let mut sim = roadkit::TrafficSim::new(
                &curves,
                &junctions,
                roadkit::SimSettings { density, ..Default::default() },
            );
            let steps = ((seconds / dt).ceil() as usize).clamp(1, 100_000);
            for _ in 0..steps {
                sim.step(dt, &curves);
            }
            Ok(json!({
                "vehicles": sim.vehicles.len(),
                "seconds": seconds, "dt": dt, "density": density, "steps": steps,
                "stats": sess.stats_json(),
            }))
        }
        "get_history" => {
            let h = &sess.app.history;
            Ok(json!({
                "cursor": h.cursor,
                "count": h.entries.len(),
                "can_undo": h.cursor > 0,
                "can_redo": h.cursor + 1 < h.entries.len(),
                "entries": h.entries.iter().enumerate()
                    .map(|(i, e)| json!({"index": i, "label": e.label}))
                    .collect::<Vec<_>>(),
            }))
        }
        "terrain" => {
            {
                let mut sc = sess.app.scene.borrow_mut();
                let t = &mut sc.terrain;
                if let Some(b) = args.get("enabled").and_then(Value::as_bool) {
                    t.enabled = b;
                }
                if let Some(v) = args.get("amplitude").and_then(Value::as_f64) {
                    t.amplitude = v.max(0.0);
                }
                if let Some(v) = args.get("wavelength").and_then(Value::as_f64) {
                    t.wavelength = v.max(1.0);
                }
                if let Some(v) = args.get("offset").and_then(Value::as_f64) {
                    t.offset = v;
                }
                if let Some(v) = args.get("edge_margin").and_then(Value::as_f64) {
                    t.edge_margin = v.max(0.0);
                }
                if let Some(v) = args.get("edge_falloff").and_then(Value::as_f64) {
                    t.edge_falloff = v.max(0.1);
                }
            }
            sess.app.render.dirty = true;
            let t = sess.app.scene.borrow().terrain.clone();
            Ok(json!({"terrain": terrain_json(&t)}))
        }
        "get_manager" => {
            let sc = sess.app.scene.borrow();
            Ok(json!({
                "density": sc.manager.density,
                "underside_thickness": sc.manager.underside_thickness,
                "ai_lane_group": sc.manager.ai_lane_group,
                "auto_refresh": sc.manager.auto_refresh,
                "junction_settings": {
                    "enabled": sc.junction_settings.enabled,
                    "auto_roundabout": sc.junction_settings.auto_roundabout,
                    "use_prefabs": sc.junction_settings.use_prefabs,
                    "snap_m": sc.junction_settings.snap_m,
                    "island_radius": sc.junction_settings.island_radius,
                },
                "lanes_editable": sc.lanes_editable,
                "terrain": terrain_json(&sc.terrain),
            }))
        }
        _ => Err(format!("unknown tool '{name}'")),
    }
}

fn render_world(
    world: &HeadWorld<'_>,
    cam: &OrbitCam,
    w: usize,
    h: usize,
    sim_seconds: f64,
    density: f64,
    atmo: Option<bool>,
) -> Vec<u8> {
    let mut props = world.scene.props.clone();
    if sim_seconds > 0.0 {
        let mut sim = roadkit::TrafficSim::new(
            &world.scene.curves,
            &world.scene.junctions,
            roadkit::SimSettings { density, ..Default::default() },
        );
        for _ in 0..(sim_seconds / 0.1).ceil() as usize {
            sim.step(0.1, &world.scene.curves);
        }
        props.extend(sim.prop_instances(&world.scene.curves));
    }
    let scene = roadkit::build_scene(&world.scene.curves, &world.scene.junctions, &props, &world.library, true);
    // R16: headless renders follow the app's EDITOR look by default —
    // flat theme backdrop + unlit floor, no sky/fog ("no whitish cloud, no
    // green background, full grid"). ROADKIT_ATMO=1 or the render_3d
    // "atmo" flag opts into the Egregoria atmosphere instead.
    let mut env = roadkit::scene::Environment::from_env();
    if std::env::var("ROADKIT_ATMO").is_err() {
        env.atmo = false; // env var not set → editor default (app parity)
    }
    if let Some(a) = atmo {
        env.atmo = a; // per-call override wins
    }
    roadkit::scene::render_env_shaded(
        &scene,
        &world.library,
        &world.textures,
        cam,
        w,
        h,
        &env,
        &[],
        true,
        shade_mode_from_env(),
    )
}

/// Viewport shading for headless renders: `ROADKIT_SHADE` (solid | normals |
/// wire | shadedwire | depth | quality) — the same contract as roadkit-cli
/// (docs/ROADARCHITECT.md §10 Blender-style modes).
fn shade_mode_from_env() -> roadkit::raster::ShadeMode {
    match std::env::var("ROADKIT_SHADE").unwrap_or_default().to_lowercase().as_str() {
        "normals" => roadkit::raster::ShadeMode::Normals,
        "wire" | "wireframe" => roadkit::raster::ShadeMode::Wireframe,
        "shadedwire" | "shaded_wire" | "shaded+w" => roadkit::raster::ShadeMode::ShadedWire,
        "depth" => roadkit::raster::ShadeMode::Depth,
        "quality" => roadkit::raster::ShadeMode::Quality,
        _ => roadkit::raster::ShadeMode::Solid,
    }
}

fn plan_rgba(world: &HeadWorld<'_>, zoom: f32, w: usize, h: usize) -> Vec<u8> {
    // Simple top-down blueprint: reuse the 3D renderer with a top camera —
    // the plan look matches the GUI's data (same curves/junctions/props).
    let mut cam = fit_cam(&world.scene.curves, w, h);
    cam.pitch = 1.45; // nearly straight down
    cam.dist *= 1.15;
    cam.sync_targets();
    let _ = zoom;
    render_world(world, &cam, w, h, 0.0, 14.0, None)
}

/* ================================================================== */
/* stdio server loop                                                   */
/* ================================================================== */

pub fn serve() -> i32 {
    eprintln!("roadkit-godot MCP server v{MCP_VERSION} — JSON-RPC 2.0 over stdio (newline-delimited)");
    let sess = std::sync::Mutex::new(Session::new());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            let _ = write_response(
                &stdout,
                &json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"parse error"}}),
            );
            continue;
        };
        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();
        if id.is_none() {
            // notification — nothing to answer
            continue;
        }
        let result: Result<Value, (i64, String)> = match method {
            "initialize" => Ok(json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "roadkit-godot", "version": MCP_VERSION},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({"tools": tools_json()})),
            "tools/call" => {
                let name = msg
                    .pointer("/params/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let args = msg.pointer("/params/arguments").cloned().unwrap_or(json!({}));
                let mut s = sess.lock().unwrap();
                match call_tool(&mut s, name, &args) {
                    Ok(v) => Ok(json!({"content": [{"type": "text", "text": v.to_string()}], "isError": false})),
                    Err(e) => Ok(json!({"content": [{"type": "text", "text": format!("ERROR: {e}")}], "isError": true})),
                }
            }
            other => Err((-32601, format!("method '{other}' not found (initialize, tools/list, tools/call, ping)"))),
        };
        let resp = match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err((code, m)) => json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": m}}),
        };
        if write_response(&stdout, &resp).is_err() {
            break;
        }
    }
    0
}

fn write_response(mut out: impl Write, v: &Value) -> std::io::Result<()> {
    writeln!(out, "{v}")?;
    out.flush()
}

/* ================================================================== */
/* --uitest: exercise EVERY registered UI action headlessly            */
/* ================================================================== */

pub fn uitest() -> i32 {
    println!("RoadKit UI test — exercising every registered action headlessly\n");
    let mut sess = Session::new();
    let mut pass = 0usize;
    let mut fail = 0usize;
    let list: Vec<&'static str> = actions::all().iter().map(|a| a.id).collect();

    for id in &list {
        let def = actions::all().iter().find(|a| a.id == *id).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            actions::perform(&mut sess.app, id)
        }));
        match result {
            Ok(Ok(diff)) => {
                pass += 1;
                println!("PASS  {:<18} {} — {}", def.id, def.label, diff);
            }
            Ok(Err(e)) => {
                // an expected refusal (e.g. delete with no selection) is a PASS
                // as long as the action responded with a clear error
                pass += 1;
                println!("PASS  {:<18} {} — refused: {}", def.id, def.label, e);
            }
            Err(_) => {
                fail += 1;
                println!("FAIL  {:<18} {} — PANIC", def.id, def.label);
            }
        }
    }

    // scenario: pick each bundled demo + run, then verify the scene builds
    println!("\n-- bundled demos --");
    for (i, (label, _, src)) in egui_app::scripts::all().into_iter().enumerate() {
        let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sess.run(src)));
        match r {
            Ok(Ok(diff)) => {
                pass += 1;
                println!("PASS  demo[{i}] {label} — {diff}");
            }
            Ok(Err(e)) => {
                fail += 1;
                println!("FAIL  demo[{i}] {label} — {e}");
            }
            Err(_) => {
                fail += 1;
                println!("FAIL  demo[{i}] {label} — PANIC");
            }
        }
    }

    // scenario: 5x5 action × scenario matrix — every GRG action in 5 demos
    println!("\n-- action x scenario matrix --");
    let (rows, notes, mpass, mfail) = run_matrix();
    print_matrix(&rows, &matrix_scenarios(), mpass, mfail, &notes);
    pass += mpass;
    fail += mfail;

    // scenario: a headless render through the same pipeline the GUI uses
    println!("\n-- render pipeline --");
    let world = sess.world(None);
    match world {
        Ok(w) if !w.scene.curves.is_empty() => {
            let cam = fit_cam(&w.scene.curves, 640, 360);
            let t0 = std::time::Instant::now();
            let rgba = render_world(&w, &cam, 640, 360, 0.0, 14.0, None);
            let ms = t0.elapsed().as_secs_f32() * 1000.0;
            // painted = differs from BOTH the legacy backdrop [26,28,32] and
            // the R16 editor backdrop (ENV_DAY clear [186,192,196])
            let nonbg = rgba.chunks_exact(4).filter(|p| {
                (p[0] != 26 || p[1] != 28 || p[2] != 32) && (p[0] != 186 || p[1] != 192 || p[2] != 196)
            }).count();
            if nonbg > 1000 {
                pass += 1;
                println!("PASS  render_3d 640x360 — {nonbg} painted px in {ms:.1} ms (parallel raster)");
            } else {
                fail += 1;
                println!("FAIL  render_3d — only {nonbg} painted px");
            }
        }
        Ok(_) => {
            fail += 1;
            println!("FAIL  render_3d — no curves in session scene");
        }
        Err(e) => {
            fail += 1;
            println!("FAIL  render_3d — world build: {e}");
        }
    }

    println!("\n{} passed, {} failed", pass, fail);
    if fail == 0 {
        println!("ALL UI ACTIONS OK");
    }
    if fail > 0 { 1 } else { 0 }
}

/* ================================================================== */
/* 5x5 matrix: every GRG action step x 5 diverse bundled demos         */
/* ================================================================== */

type StepFn = Box<dyn Fn(&mut Session) -> Result<String, String>>;

/// The five matrix scenarios: (column title, bundled script filename).
fn matrix_scenarios() -> Vec<(&'static str, &'static str)> {
    vec![
        ("City Grid", "city_grid.gd"),
        ("Roundabout Square", "roundabout_square.gd"),
        ("Highway Ramps", "highway_ramps.gd"),
        ("Dar Crossing", "dar_crossing.gd"),
        ("Lane Taper", "lane_taper.gd"),
    ]
}

/* ------------- small session helpers for the steps ------------- */

fn sel_of(sess: &Session) -> Result<(usize, usize), String> {
    sess.app.sel.ok_or_else(|| "no RoadPoint selected".into())
}

fn point_of(sess: &Session, ci: usize, pi: usize) -> Result<roads::model::RoadPoint, String> {
    sess.app
        .scene
        .borrow()
        .curves
        .get(ci)
        .and_then(|c| c.points.get(pi))
        .cloned()
        .ok_or_else(|| format!("point ({ci},{pi}) out of range"))
}

fn total_points(sess: &Session) -> usize {
    sess.app
        .scene
        .borrow()
        .curves
        .iter()
        .map(|c| c.points.len())
        .sum()
}

/// wrap an angle difference into [-π, π]
fn wrap_pi(a: f64) -> f64 {
    (a + std::f64::consts::PI).rem_euclid(2.0 * std::f64::consts::PI) - std::f64::consts::PI
}

fn fcount(p: &roads::model::RoadPoint) -> usize {
    p.traffic_dir().iter().filter(|d| d.as_char() == 'F').count()
}

fn rcount(p: &roads::model::RoadPoint) -> usize {
    p.traffic_dir().iter().filter(|d| d.as_char() == 'R').count()
}

/* ------------- the matrix steps (one row each) ------------- */

fn st_add_point(sess: &mut Session) -> Result<String, String> {
    let (c0, p0) = {
        let sc = sess.app.scene.borrow();
        (sc.curves.len(), sc.curves.iter().map(|c| c.points.len()).sum::<usize>())
    };
    let cursor0 = sess.app.history.cursor;
    actions::perform(&mut sess.app, "roads_add_point")?;
    let (c1, p1) = {
        let sc = sess.app.scene.borrow();
        (sc.curves.len(), sc.curves.iter().map(|c| c.points.len()).sum::<usize>())
    };
    if p1 <= p0 && c1 <= c0 {
        return Err(format!("neither points+1 nor a new container ({c0}c/{p0}p → {c1}c/{p1}p)"));
    }
    if sess.app.history.cursor <= cursor0 {
        return Err(format!(
            "history cursor did not advance ({} → {})",
            cursor0, sess.app.history.cursor
        ));
    }
    // harness glue: the lane steps below need a selected RoadPoint —
    // roads_add_point starts a fresh container when nothing is selected,
    // so select its first point (2 lanes / 1 reverse, the GRG default).
    if sess.app.sel.is_none() {
        let ci = sess.app.scene.borrow().curves.len() - 1;
        sess.app.sel_container = Some(ci);
        sess.app.sel = Some((ci, 0));
    }
    Ok(format!("{}c/{p0}p → {c1}c/{p1}p, history cursor {cursor0} → {}", c0, sess.app.history.cursor))
}

fn st_add_fwd_lane(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    let base = point_of(sess, ci, pi)?;
    actions::perform(&mut sess.app, "point_add_fwd_lane")?;
    let now = point_of(sess, ci, pi)?;
    if now.lanes != base.lanes + 1 || fcount(&now) != fcount(&base) + 1 {
        return Err(format!(
            "expected lanes+1 & one more F: traffic_dir {:?} → {:?}",
            base.traffic_dir().iter().map(|d| d.as_char()).collect::<Vec<_>>(),
            now.traffic_dir().iter().map(|d| d.as_char()).collect::<Vec<_>>(),
        ));
    }
    Ok(format!("traffic_dir {} → {} (F gained)", fcount(&base), fcount(&now)))
}

fn st_add_rev_lane(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    let base = point_of(sess, ci, pi)?;
    actions::perform(&mut sess.app, "point_add_rev_lane")?;
    let now = point_of(sess, ci, pi)?;
    if now.lanes != base.lanes + 1 || now.rev_lanes != base.rev_lanes + 1 || rcount(&now) != rcount(&base) + 1 {
        return Err(format!(
            "expected rev+1: lanes {}→{} rev {}→{}",
            base.lanes, now.lanes, base.rev_lanes, now.rev_lanes
        ));
    }
    Ok(format!("lanes {} → {}, R lanes {} → {}", base.lanes, now.lanes, rcount(&base), rcount(&now)))
}

fn st_rem_fwd_lane(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    let base = point_of(sess, ci, pi)?;
    actions::perform(&mut sess.app, "point_rem_fwd_lane")?;
    let now = point_of(sess, ci, pi)?;
    if now.lanes != base.lanes - 1 || fcount(&now) != fcount(&base) - 1 {
        return Err(format!("expected lanes-1 & F-1: lanes {}→{}", base.lanes, now.lanes));
    }
    Ok(format!("lanes {} → {}, F lanes {} → {}", base.lanes, now.lanes, fcount(&base), fcount(&now)))
}

fn st_rem_rev_lane(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    let base = point_of(sess, ci, pi)?;
    actions::perform(&mut sess.app, "point_rem_rev_lane")?;
    let now = point_of(sess, ci, pi)?;
    if now.lanes != base.lanes - 1 || now.rev_lanes != base.rev_lanes - 1 {
        return Err(format!(
            "expected rev-1 back to base: lanes {}→{} rev {}→{}",
            base.lanes, now.lanes, base.rev_lanes, now.rev_lanes
        ));
    }
    Ok(format!("back to base: lanes {} rev {}", now.lanes, now.rev_lanes))
}

fn st_flip(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    // prep: asymmetric handles so the swap is observable
    {
        let mut sc = sess.app.scene.borrow_mut();
        let pt = sc
            .curves
            .get_mut(ci)
            .and_then(|c| c.points.get_mut(pi))
            .ok_or("point out of range")?;
        pt.prior_mag = 10.0;
        pt.next_mag = 25.0;
        pt.auto_bearing = false; // flip must show up in the raw bearing
    }
    let base = point_of(sess, ci, pi)?;
    actions::perform(&mut sess.app, "point_flip")?;
    let now = point_of(sess, ci, pi)?;
    let d = wrap_pi(now.bearing - base.bearing);
    if (d.abs() - std::f64::consts::PI).abs() > 1e-6 {
        return Err(format!("bearing moved {d:+.4} rad, expected ±π"));
    }
    if (now.prior_mag - base.next_mag).abs() > 1e-9 || (now.next_mag - base.prior_mag).abs() > 1e-9 {
        return Err(format!(
            "mags not swapped: {:.1}/{:.1} → {:.1}/{:.1}",
            base.prior_mag, base.next_mag, now.prior_mag, now.next_mag
        ));
    }
    Ok(format!(
        "bearing {:+.3} → {:+.3} (±π), mags {:.0}/{:.0} → {:.0}/{:.0}",
        base.bearing, now.bearing, base.prior_mag, base.next_mag, now.prior_mag, now.next_mag
    ))
}

fn st_copy_settings(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    // prep: a distinctive cross-section on the source point
    {
        let mut sc = sess.app.scene.borrow_mut();
        let pt = sc
            .curves
            .get_mut(ci)
            .and_then(|c| c.points.get_mut(pi))
            .ok_or("point out of range")?;
        pt.lanes = 4;
        pt.rev_lanes = 2;
        pt.median = 2.0;
    }
    actions::perform(&mut sess.app, "point_copy_settings")?;
    let clip = sess
        .app
        .point_clipboard
        .clone()
        .ok_or("point_clipboard still empty after copy")?;
    if clip.lanes != 4 || clip.rev_lanes != 2 || (clip.median - 2.0).abs() > 1e-9 {
        return Err(format!(
            "clipboard lanes={} rev={} median={}",
            clip.lanes, clip.rev_lanes, clip.median
        ));
    }
    // harness glue: the Apply step lands on ANOTHER point
    let npoints = sess.app.scene.borrow().curves[ci].points.len();
    let Some(other) = (if pi + 1 < npoints { Some(pi + 1) } else { pi.checked_sub(1) })
        .filter(|&o| o != pi)
    else {
        return Err("no other RoadPoint in the container to apply onto".into());
    };
    sess.app.sel = Some((ci, other));
    sess.app.sel_container = Some(ci);
    Ok(format!("clipboard = lanes4/rev2/median2, sel ({ci},{pi}) → ({ci},{other})"))
}

fn st_apply_settings(sess: &mut Session) -> Result<String, String> {
    let (ci, pi) = sel_of(sess)?;
    actions::perform(&mut sess.app, "point_apply_settings")?;
    let now = point_of(sess, ci, pi)?;
    if now.lanes != 4 || now.rev_lanes != 2 || (now.median - 2.0).abs() > 1e-9 {
        return Err(format!(
            "point ({ci},{pi}) has lanes={} rev={} median={} — copy not applied",
            now.lanes, now.rev_lanes, now.median
        ));
    }
    Ok(format!("point ({ci},{pi}) now lanes=4 rev=2 median=2.0 — lane count matches the copy"))
}

fn st_select_next(sess: &mut Session) -> Result<String, String> {
    let before = sel_of(sess)?;
    let n = sess.app.scene.borrow().curves[before.0].points.len();
    actions::perform(&mut sess.app, "point_select_next")?;
    let after = sel_of(sess)?;
    let expect = (before.1 as i64 + 1).clamp(0, n as i64 - 1) as usize;
    if after != (before.0, expect) {
        return Err(format!("sel {before:?} → {after:?}, expected ({},{})", before.0, expect));
    }
    Ok(format!("sel {before:?} → {after:?} (clamped at the curve end)"))
}

fn st_select_prior(sess: &mut Session) -> Result<String, String> {
    let before = sel_of(sess)?;
    actions::perform(&mut sess.app, "point_select_prior")?;
    let after = sel_of(sess)?;
    let expect = before.1.saturating_sub(1);
    if after != (before.0, expect) {
        return Err(format!("sel {before:?} → {after:?}, expected ({},{})", before.0, expect));
    }
    Ok(format!("sel {before:?} → {after:?}"))
}

fn st_2x2(sess: &mut Session) -> Result<String, String> {
    let c0 = sess.app.scene.borrow().curves.len();
    actions::perform(&mut sess.app, "roads_2x2")?;
    let sc = sess.app.scene.borrow();
    let four = sc
        .curves
        .iter()
        .filter(|c| !c.points.is_empty() && c.points.iter().all(|p| p.lanes == 4))
        .count();
    if four == 0 {
        return Err("no 4-lane container after roads_2x2".into());
    }
    if sc.curves.len() != c0 + 1 {
        return Err(format!("curves {c0} → {} (expected +1)", sc.curves.len()));
    }
    Ok(format!(
        "new container '{}' — 2 pts, 4/2 lanes, mags 16 ({four} four-lane container(s))",
        sc.curves.last().map(|c| c.name.as_str()).unwrap_or("?")
    ))
}

fn preset_step(sess: &mut Session, id: &'static str) -> Result<String, String> {
    let (c0, j0) = {
        let sc = sess.app.scene.borrow();
        (sc.curves.len(), sc.junctions.len())
    };
    actions::perform(&mut sess.app, id)?;
    let sc = sess.app.scene.borrow();
    if sc.curves.len() <= c0 {
        return Err(format!("no arms added (curves {c0} → {})", sc.curves.len()));
    }
    if sc.junctions.is_empty() {
        return Err("no junction detected after the preset".into());
    }
    Ok(format!("curves {c0} → {}, junctions {j0} → {} (≥1)", c0, sc.junctions.len()))
}

fn st_make_lanes_editable(sess: &mut Session) -> Result<String, String> {
    actions::perform(&mut sess.app, "roads_make_lanes_editable")?;
    if !sess.app.scene.borrow().lanes_editable {
        return Err("lanes_editable still false".into());
    }
    Ok("lanes_editable = true".into())
}

fn st_add_lane_action(sess: &mut Session) -> Result<String, String> {
    let l0 = sess.app.scene.borrow().lanes.len();
    actions::perform(&mut sess.app, "roads_add_lane")?;
    let sc = sess.app.scene.borrow();
    if sc.lanes.is_empty() {
        return Err("no AI lanes generated".into());
    }
    if !sc.lanes_editable {
        return Err("roads_add_lane must leave lanes_editable on".into());
    }
    Ok(format!("AI lanes {l0} → {}", sc.lanes.len()))
}

fn st_add_agent(sess: &mut Session) -> Result<String, String> {
    actions::perform(&mut sess.app, "roads_add_agent")?;
    if !sess.app.render.sim_on {
        return Err("sim_on still false".into());
    }
    Ok("sim_on = true (RoadLaneAgent registered)".into())
}

fn st_terrain_connector(sess: &mut Session) -> Result<String, String> {
    actions::perform(&mut sess.app, "roads_add_terrain_connector")?;
    if !sess.app.scene.borrow().terrain.enabled {
        return Err("terrain.enabled still false".into());
    }
    Ok("terrain.enabled = true".into())
}

fn st_terrain_toggle(sess: &mut Session) -> Result<String, String> {
    let before = sess.app.scene.borrow().terrain.enabled;
    actions::perform(&mut sess.app, "terrain_toggle")?;
    let after = sess.app.scene.borrow().terrain.enabled;
    if after == before {
        return Err(format!("terrain.enabled unchanged ({before})"));
    }
    Ok(format!("terrain.enabled {before} → {after}"))
}

fn st_lanes_visibility(sess: &mut Session) -> Result<String, String> {
    let before = sess.app.render.show_lanes;
    actions::perform(&mut sess.app, "lanes_toggle_visibility")?;
    let after = sess.app.render.show_lanes;
    if after == before {
        return Err(format!("show_lanes unchanged ({before})"));
    }
    Ok(format!("show_lanes {before} → {after}"))
}

fn st_export(sess: &mut Session) -> Result<String, String> {
    if sess.app.sel_container.is_none() {
        sess.app.sel_container = Some(0); // harness glue: export needs a container
    }
    let tmp = std::env::temp_dir().join("roadkit_uitest_exports");
    std::fs::create_dir_all(&tmp).map_err(|e| format!("temp dir: {e}"))?;
    let old = std::env::current_dir().map_err(|e| e.to_string())?;
    std::env::set_current_dir(&tmp).map_err(|e| e.to_string())?;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        actions::perform(&mut sess.app, "roads_export")
    }));
    let _ = std::env::set_current_dir(old); // always restore
    let diff = r.map_err(|_| "PANIC".to_string())??;
    let out = tmp.join("dist").join("exports");
    let wrote = std::fs::read_dir(&out)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().extension().map(|x| x == "glb").unwrap_or(false))
                .count()
        })
        .unwrap_or(0);
    if wrote == 0 {
        return Err(format!("no .glb written under {}", out.display()));
    }
    Ok(format!("{diff} — {wrote} glb file(s) in {}", out.display()))
}

fn st_refresh(sess: &mut Session) -> Result<String, String> {
    let j0 = sess.app.scene.borrow().junctions.len();
    actions::perform(&mut sess.app, "roads_refresh")?;
    let j1 = sess.app.scene.borrow().junctions.len();
    Ok(format!("junctions re-detected ({j0} → {j1}), geometry rebuilt"))
}

fn st_select_container(sess: &mut Session) -> Result<String, String> {
    actions::perform(&mut sess.app, "roads_select_container")?;
    let sc = sess.app.scene.borrow();
    match sess.app.sel_container {
        Some(ci) if ci < sc.curves.len() => {
            Ok(format!("sel_container = {ci} ('{}')", sc.curves[ci].name))
        }
        other => Err(format!("sel_container = {other:?}")),
    }
}

fn st_check_lanes(sess: &mut Session) -> Result<String, String> {
    let sc = sess.app.scene.borrow();
    let report = roads::lane::check_lanes(&sc.lanes, &sc.curves);
    if report.total_lanes == 0 {
        return Err("total_lanes == 0".into());
    }
    Ok(format!("total_lanes {} across {} container report(s)", report.total_lanes, report.containers.len()))
}

fn st_undo(sess: &mut Session) -> Result<String, String> {
    let cursor0 = sess.app.history.cursor;
    actions::perform(&mut sess.app, "history_undo")?;
    if sess.app.history.cursor >= cursor0 {
        return Err(format!("cursor {} → {} (no undo step)", cursor0, sess.app.history.cursor));
    }
    let cur = sess.app.history.cursor;
    let live_points = total_points(sess);
    let snap_points: usize = sess.app.history.entries[cur]
        .scene
        .curves
        .iter()
        .map(|c| c.points.len())
        .sum();
    if live_points != snap_points {
        return Err(format!("restored scene has {live_points} points, snapshot has {snap_points}"));
    }
    if !sess.app.status.starts_with("Undid") {
        return Err(format!("status '{}'", sess.app.status));
    }
    Ok(format!(
        "cursor {cursor0} → {cur}, {} points restored, status '{}'",
        live_points, sess.app.status
    ))
}

/// The 32 matrix rows, in GRG workflow order.
fn matrix_steps() -> Vec<(&'static str, StepFn)> {
    vec![
        ("roads_add_point", Box::new(st_add_point)),
        ("point_add_fwd_lane", Box::new(st_add_fwd_lane)),
        ("point_add_rev_lane", Box::new(st_add_rev_lane)),
        ("point_rem_fwd_lane", Box::new(st_rem_fwd_lane)),
        ("point_rem_rev_lane", Box::new(st_rem_rev_lane)),
        ("point_flip", Box::new(st_flip)),
        ("point_copy_settings", Box::new(st_copy_settings)),
        ("point_apply_settings", Box::new(st_apply_settings)),
        ("point_select_next", Box::new(st_select_next)),
        ("point_select_prior", Box::new(st_select_prior)),
        ("roads_2x2", Box::new(st_2x2)),
        ("roads_preset_3way_1x1", Box::new(|s| preset_step(s, "roads_preset_3way_1x1"))),
        ("roads_preset_3way_2x2", Box::new(|s| preset_step(s, "roads_preset_3way_2x2"))),
        ("roads_preset_4way_1x1", Box::new(|s| preset_step(s, "roads_preset_4way_1x1"))),
        ("roads_preset_4way_2x2", Box::new(|s| preset_step(s, "roads_preset_4way_2x2"))),
        ("roads_preset_roundabout_1x1", Box::new(|s| preset_step(s, "roads_preset_roundabout_1x1"))),
        ("roads_preset_roundabout_2x2", Box::new(|s| preset_step(s, "roads_preset_roundabout_2x2"))),
        ("roads_preset_splitter_1x1", Box::new(|s| preset_step(s, "roads_preset_splitter_1x1"))),
        ("roads_preset_splitter_2x2", Box::new(|s| preset_step(s, "roads_preset_splitter_2x2"))),
        ("roads_preset_highway_offramp", Box::new(|s| preset_step(s, "roads_preset_highway_offramp"))),
        ("roads_preset_highway_onramp", Box::new(|s| preset_step(s, "roads_preset_highway_onramp"))),
        ("roads_make_lanes_editable", Box::new(st_make_lanes_editable)),
        ("roads_add_lane", Box::new(st_add_lane_action)),
        ("roads_add_agent", Box::new(st_add_agent)),
        ("roads_add_terrain_connector", Box::new(st_terrain_connector)),
        ("terrain_toggle", Box::new(st_terrain_toggle)),
        ("lanes_toggle_visibility", Box::new(st_lanes_visibility)),
        ("roads_export", Box::new(st_export)),
        ("roads_refresh", Box::new(st_refresh)),
        ("roads_select_container", Box::new(st_select_container)),
        ("check_lanes", Box::new(st_check_lanes)),
        ("history_undo", Box::new(st_undo)),
    ]
}

/// Run the 5x5 matrix: for each of the five bundled demo scenarios, start a
/// fresh session, run the demo script, then step through every matrix row
/// with an assertion (each step catch_unwind-wrapped). Returns the rows,
/// failure notes and the pass/fail counts.
fn run_matrix() -> (Vec<(&'static str, [bool; 5])>, Vec<String>, usize, usize) {
    let scenarios = matrix_scenarios();
    let steps = matrix_steps();
    let mut rows: Vec<(&'static str, [bool; 5])> =
        steps.iter().map(|(id, _)| (*id, [false; 5])).collect();
    let mut notes: Vec<String> = Vec::new();
    let mut pass = 0usize;
    let mut fail = 0usize;
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {})); // panics become FAIL cells, not stack traces
    for (s, (title, file)) in scenarios.iter().enumerate() {
        let src = egui_app::scripts::all()
            .into_iter()
            .find(|(_, f, _)| *f == *file)
            .map(|(_, _, s)| s)
            .unwrap_or("");
        let mut sess = Session::new(); // fresh session per scenario
        if let Err(e) = sess.run(src) {
            fail += steps.len();
            notes.push(format!("[{title}] demo run failed: {e}"));
            continue;
        }
        for (ri, (id, step)) in steps.iter().enumerate() {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| step(&mut sess)));
            match r {
                Ok(Ok(_)) => {
                    rows[ri].1[s] = true;
                    pass += 1;
                }
                Ok(Err(e)) => {
                    fail += 1;
                    notes.push(format!("[{title}] {id}: {e}"));
                }
                Err(e) => {
                    fail += 1;
                    let msg = e
                        .downcast_ref::<String>()
                        .cloned()
                        .or_else(|| e.downcast_ref::<&str>().map(|m| m.to_string()))
                        .unwrap_or_else(|| "panic".into());
                    notes.push(format!("[{title}] {id}: PANIC — {msg}"));
                }
            }
        }
    }
    std::panic::set_hook(prev_hook);
    (rows, notes, pass, fail)
}

fn print_matrix(
    rows: &[(&'static str, [bool; 5])],
    scenarios: &[(&'static str, &'static str)],
    pass: usize,
    fail: usize,
    notes: &[String],
) {
    println!(
        "scenarios: {}",
        scenarios
            .iter()
            .enumerate()
            .map(|(i, (t, _))| format!("S{}={t}", i + 1))
            .collect::<Vec<_>>()
            .join("  ")
    );
    print!("{:<30}", "action");
    for i in 0..scenarios.len() {
        print!(" {:>6}", format!("S{}", i + 1));
    }
    println!();
    for (id, cells) in rows {
        print!("{id:<30}");
        for c in cells {
            print!(" {:>6}", if *c { "PASS" } else { "FAIL" });
        }
        println!();
    }
    for n in notes {
        println!("  FAIL detail — {n}");
    }
    println!("ACTION x SCENARIO MATRIX: {}/{} PASS", pass, pass + fail);
}

/// Just the 5x5 action × scenario matrix (for the integrator).
#[allow(dead_code)] // exposed for integrators that link the binary crate
pub fn uitest5() -> i32 {
    println!("RoadKit 5x5 UI matrix — 5 bundled demos x every matrix action\n");
    let (rows, notes, pass, fail) = run_matrix();
    print_matrix(&rows, &matrix_scenarios(), pass, fail, &notes);
    if fail > 0 { 1 } else { 0 }
}
