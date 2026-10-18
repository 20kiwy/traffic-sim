//! Central UI action registry — ONE implementation per button, shared by the
//! GUI chrome, the headless `--uitest` runner and the MCP server's
//! `list_actions` / `do_action` tools. "Test all buttons" means calling every
//! entry here and diffing the state.

use crate::state::{DockTab, RoadkitApp, ScriptStatus, Tab, Tool};
use roads::model::TrafficUpdate;

pub struct ActionDef {
    pub id: &'static str,
    pub label: &'static str,
    /// where the control lives in the UI
    pub location: &'static str,
    pub hotkey: Option<&'static str>,
}

/// Every clickable top-level control in the app (tool rail, bars, menus,
/// dock tabs, 3D toolbar). Row-level controls (eye/lock per layer, per-asset
/// stamp buttons, inspector fields) are data-driven and stay in panels.rs.
pub fn all() -> &'static [ActionDef] {
    &ACTIONS
}

static ACTIONS: &[ActionDef] = &[
    // tools (tool rail)
    ActionDef { id: "tool_select", label: "Select / move RoadPoints", location: "tool rail", hotkey: Some("V") },
    ActionDef { id: "tool_add", label: "Add RoadPoint on the plan", location: "tool rail", hotkey: Some("A") },
    ActionDef { id: "tool_stamp", label: "Stamp selected GLB prop", location: "tool rail", hotkey: Some("P") },
    ActionDef { id: "tool_pan", label: "Pan the 2D plan", location: "tool rail", hotkey: Some("H") },
    // script + scene
    ActionDef { id: "run_script", label: "Run the .gd script", location: "control bar ▶ Run / Script menu / tool rail", hotkey: Some("F5") },
    ActionDef { id: "reset_scene", label: "Reset scene (clear everything)", location: "Script menu / Script panel", hotkey: None },
    ActionDef { id: "join_props", label: "Join GLB props like Godot (bulk dressing)", location: "Props menu / Assets panel", hotkey: None },
    ActionDef { id: "clear_props", label: "Clear all placed GLB props", location: "Props menu / Assets panel", hotkey: None },
    ActionDef { id: "add_road", label: "New RoadContainer with 2 points", location: "Layers panel +", hotkey: None },
    ActionDef { id: "delete_selected", label: "Delete selected RoadPoint / container", location: "Properties panel / Del", hotkey: Some("Del") },
    ActionDef { id: "script_next", label: "Next bundled demo script", location: "Script panel combo", hotkey: None },
    ActionDef { id: "script_prev", label: "Previous bundled demo script", location: "Script panel combo", hotkey: None },
    // views
    ActionDef { id: "tab_3d", label: "Show 3D Scene tab", location: "control bar / doc tabs / tool rail", hotkey: None },
    ActionDef { id: "tab_2d", label: "Show 2D Plan tab", location: "control bar / doc tabs / tool rail", hotkey: None },
    ActionDef { id: "fit_scene", label: "Fit the scene in view", location: "control bar / tool rail / View menu", hotkey: Some("F") },
    ActionDef { id: "reset_camera", label: "Reset the 3D orbit camera", location: "control bar / View menu", hotkey: Some("R") },
    // 3D toolbar toggles
    ActionDef { id: "toggle_grid", label: "Toggle ground grid", location: "3D toolbar", hotkey: None },
    ActionDef { id: "toggle_props", label: "Toggle GLB props visibility", location: "3D toolbar", hotkey: None },
    ActionDef { id: "toggle_spin", label: "Toggle auto-orbit camera", location: "3D toolbar", hotkey: None },
    ActionDef { id: "toggle_traffic", label: "Toggle traffic simulation", location: "3D toolbar", hotkey: None },
    ActionDef { id: "rebuild_scene", label: "Rebuild the 3D scene mesh", location: "3D toolbar camera button", hotkey: None },
    // junction engine (control bar checkboxes)
    ActionDef { id: "toggle_autojoin", label: "Toggle auto-join endpoints", location: "control bar", hotkey: None },
    ActionDef { id: "toggle_roundabouts", label: "Toggle roundabout detection", location: "control bar", hotkey: None },
    ActionDef { id: "toggle_prefabs", label: "Toggle WFC GLB junction prefabs", location: "control bar", hotkey: None },
    // dock panels (icon column)
    ActionDef { id: "panel_layers", label: "Toggle Layers panel", location: "icon column", hotkey: None },
    ActionDef { id: "panel_properties", label: "Toggle Properties panel", location: "icon column", hotkey: None },
    ActionDef { id: "panel_assets", label: "Toggle Assets panel", location: "icon column", hotkey: None },
    ActionDef { id: "panel_script", label: "Toggle Script panel", location: "icon column", hotkey: None },
    ActionDef { id: "panel_console", label: "Toggle Console panel", location: "icon column", hotkey: None },
    ActionDef { id: "dock_collapse", label: "Collapse the right dock", location: "dock » button", hotkey: None },
    ActionDef { id: "dock_expand", label: "Re-open the collapsed dock", location: "icon column", hotkey: None },
    // dialogs
    ActionDef { id: "show_api", label: "Show .gd scripting API reference", location: "Help menu / Script panel", hotkey: None },
    ActionDef { id: "show_about", label: "Show About dialog", location: "Help menu", hotkey: None },
    ActionDef { id: "close_dialogs", label: "Close open dialogs", location: "dialog ✕", hotkey: None },
    // ---- Roads menu (the GRG plugin's "Roads" app-menu clone) ----
    ActionDef { id: "roads_refresh", label: "Refresh roads (rebuild all geometry)", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_select_container", label: "Select the container of the current selection", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_add_container", label: "Add RoadContainer", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_add_point", label: "Add next RoadPoint", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_add_lane", label: "Add RoadLane (AI path)", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_add_agent", label: "Add RoadLaneAgent (AI)", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_add_terrain_connector", label: "Add RoadTerrain3DConnector", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_2x2", label: "Add 2x2 road segment", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_preset_3way_1x1", label: "Preset RoadContainer 3way_1x1", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_3way_2x2", label: "Preset RoadContainer 3way_2x2", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_4way_1x1", label: "Preset RoadContainer 4way_1x1", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_4way_2x2", label: "Preset RoadContainer 4way_2x2", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_highway_offramp", label: "Preset RoadContainer highway_offramp", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_highway_onramp", label: "Preset RoadContainer highway_onramp", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_roundabout_1x1", label: "Preset RoadContainer roundabout_1x1", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_roundabout_2x2", label: "Preset RoadContainer roundabout_2x2", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_splitter_1x1", label: "Preset RoadContainer splitter_1x1", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_preset_splitter_2x2", label: "Preset RoadContainer splitter_2x2", location: "Roads menu › RoadContainer presets", hotkey: None },
    ActionDef { id: "roads_export", label: "Export RoadContainer to .glb", location: "Roads menu / container inspector", hotkey: None },
    ActionDef { id: "roads_make_lanes_editable", label: "Make RoadLanes Editable", location: "Roads menu / container inspector", hotkey: None },
    ActionDef { id: "roads_report_issue", label: "Report issue (GitHub)", location: "Roads menu", hotkey: None },
    ActionDef { id: "roads_share_feedback", label: "Share feedback", location: "Roads menu", hotkey: None },
    // ---- RoadPoint editor (Properties panel lane row) ----
    ActionDef { id: "point_add_fwd_lane", label: "Add Forward lane", location: "Properties › RoadPoint lane row", hotkey: None },
    ActionDef { id: "point_add_rev_lane", label: "Add Reverse lane", location: "Properties › RoadPoint lane row", hotkey: None },
    ActionDef { id: "point_rem_fwd_lane", label: "Remove Forward lane", location: "Properties › RoadPoint lane row", hotkey: None },
    ActionDef { id: "point_rem_rev_lane", label: "Remove Reverse lane", location: "Properties › RoadPoint lane row", hotkey: None },
    ActionDef { id: "point_flip", label: "Flip RoadPoint", location: "Properties › RoadPoint", hotkey: None },
    ActionDef { id: "point_copy_settings", label: "Copy RoadPoint settings", location: "Properties › RoadPoint", hotkey: None },
    ActionDef { id: "point_apply_settings", label: "Apply copied settings to RoadPoint", location: "Properties › RoadPoint", hotkey: None },
    ActionDef { id: "point_apply_all", label: "Apply copied settings to all container RoadPoints", location: "Properties › RoadPoint", hotkey: None },
    ActionDef { id: "point_select_next", label: "Select Next RoadPoint", location: "Properties › RoadPoint", hotkey: None },
    ActionDef { id: "point_select_prior", label: "Select Prior RoadPoint", location: "Properties › RoadPoint", hotkey: None },
    // ---- History dock ----
    ActionDef { id: "history_undo", label: "Undo", location: "History dock / app menu", hotkey: None },
    ActionDef { id: "history_redo", label: "Redo", location: "History dock / app menu", hotkey: None },
    ActionDef { id: "history_clear", label: "Clear the undo history", location: "History dock", hotkey: None },
    // ---- terrain + lane viz ----
    ActionDef { id: "terrain_toggle", label: "Set terrain (toggle RoadTerrain3DConnector)", location: "Properties › RoadManager", hotkey: None },
    ActionDef { id: "lanes_toggle_visibility", label: "Toggle lane (AI path) visibility", location: "3D toolbar / Roads menu", hotkey: None },
];

/// Execute an action by id. Returns a human-readable diff of what changed so
/// tests (and the MCP `do_action` tool) can assert behaviour.
pub fn perform(app: &mut RoadkitApp, id: &str) -> Result<String, String> {
    let before = snapshot(app);
    let mut out = String::new();
    match id {
        "tool_select" => app.tool = Tool::Select,
        "tool_add" => app.tool = Tool::AddPoint,
        "tool_stamp" => app.tool = Tool::Stamp,
        "tool_pan" => app.tool = Tool::Pan,
        "run_script" => {
            app.run_script();
            out = match &app.script_status {
                ScriptStatus::Ok(m) => format!("script OK — {m}"),
                ScriptStatus::Err(e) => return Err(format!("script failed: {e}")),
                ScriptStatus::Idle => "idle".into(),
            };
        }
        "reset_scene" => app.reset_scene(),
        "join_props" => app.join_props_like_godot(),
        "clear_props" => app.clear_props(),
        "add_road" => app.add_road(),
        "delete_selected" => {
            let had = app.sel.is_some() || app.sel_container.is_some();
            if let Some((ci, _pi)) = app.sel {
                app.delete_selected_point();
                app.sel = if had { None } else { None };
            } else if let Some(ci) = app.sel_container {
                app.delete_container(ci);
            } else {
                return Err("nothing selected".into());
            }
        }
        "script_next" | "script_prev" => {
            let n = crate::scripts::all().len();
            let delta = if id == "script_next" { 1 } else { n - 1 };
            app.set_script((app.script_idx + delta) % n);
            out = format!("script = {}", crate::scripts::all()[app.script_idx].0);
        }
        "tab_3d" => app.tab = Tab::Scene3d,
        "tab_2d" => app.tab = Tab::Plan,
        "fit_scene" => app.fit_scene(),
        "reset_camera" => app.cam = gdmath::OrbitCam::default(),
        "toggle_grid" => {
            app.render.show_grid = !app.render.show_grid;
            app.render.dirty = true;
        }
        "toggle_props" => app.render.show_props = !app.render.show_props,
        "toggle_spin" => app.render.spin = !app.render.spin,
        "toggle_traffic" => {
            app.render.sim_on = !app.render.sim_on;
            app.render.dirty = true;
        }
        "rebuild_scene" => app.render.dirty = true,
        "toggle_autojoin" | "toggle_roundabouts" | "toggle_prefabs" => {
            let mut sc = app.scene.borrow_mut();
            match id {
                "toggle_autojoin" => sc.junction_settings.enabled = !sc.junction_settings.enabled,
                "toggle_roundabouts" => sc.junction_settings.auto_roundabout = !sc.junction_settings.auto_roundabout,
                _ => sc.junction_settings.use_prefabs = !sc.junction_settings.use_prefabs,
            }
            sc.recompute_junctions();
            drop(sc);
            app.render.dirty = true;
        }
        "panel_layers" | "panel_properties" | "panel_assets" | "panel_script" | "panel_console" => {
            let tab = match id {
                "panel_layers" => DockTab::Layers,
                "panel_properties" => DockTab::Properties,
                "panel_assets" => DockTab::Assets,
                "panel_script" => DockTab::Script,
                _ => DockTab::Console,
            };
            let was_open = app.panels.is_open(tab) && app.dock_tab == tab && !app.dock_collapsed;
            app.panels.set_open(tab, true);
            app.dock_tab = tab;
            app.dock_collapsed = false;
            if was_open {
                app.panels.set_open(tab, false); // second click closes (icon column behaviour)
                if app.dock_tab == tab {
                    app.dock_tab = app.panels.open_tabs().first().copied().unwrap_or(tab);
                }
            }
        }
        "dock_collapse" => app.dock_collapsed = true,
        "dock_expand" => app.dock_collapsed = false,
        "show_api" => app.show_api = true,
        "show_about" => app.show_about = true,
        "close_dialogs" => {
            app.show_api = false;
            app.show_about = false;
        }
        /* ---------------- Roads menu (GRG plugin clone) ---------------- */
        "roads_refresh" => {
            let mut sc = app.scene.borrow_mut();
            let n = sc.curves.len();
            sc.recompute_junctions();
            sc.dirty = true;
            sc.log(format!("[roads] refreshed {n} containers — junctions + lanes re-detected"));
            drop(sc);
            app.render.dirty = true;
            app.status = format!("refreshed {n} road containers");
            // Godot's refresh is NOT undoable — no history push
        }
        "roads_select_container" => {
            let ci = app
                .sel
                .map(|(ci, _)| ci)
                .or(app.sel_container)
                .or_else(|| app.scene.borrow().curves.first().map(|_| 0));
            let Some(ci) = ci.filter(|&ci| ci < app.scene.borrow().curves.len()) else {
                return Err("no RoadContainer in the scene".into());
            };
            let name = app.scene.borrow().curves[ci].name.clone();
            app.sel_container = Some(ci);
            app.sel_junction = None;
            app.status = format!("selected container '{name}'");
            out = format!("sel_container = {ci}");
        }
        "roads_add_container" => {
            app.add_road();
            app.push_history("Add RoadContainer");
        }
        "roads_add_point" => {
            let x = app.plan.cx;
            let z = app.plan.cz;
            app.add_point_grg(x, z);
        }
        "roads_add_lane" => app.add_lane_node(),
        "roads_add_agent" => app.add_lane_agent(),
        "roads_add_terrain_connector" => app.add_terrain_connector(),
        "roads_2x2" => app.create_2x2_road(),
        "roads_preset_3way_1x1" => app.create_preset("3way_1x1"),
        "roads_preset_3way_2x2" => app.create_preset("3way_2x2"),
        "roads_preset_4way_1x1" => app.create_preset("4way_1x1"),
        "roads_preset_4way_2x2" => app.create_preset("4way_2x2"),
        "roads_preset_highway_offramp" => app.create_preset("highway_offramp"),
        "roads_preset_highway_onramp" => app.create_preset("highway_onramp"),
        "roads_preset_roundabout_1x1" => app.create_preset("roundabout_1x1"),
        "roads_preset_roundabout_2x2" => app.create_preset("roundabout_2x2"),
        "roads_preset_splitter_1x1" => app.create_preset("splitter_1x1"),
        "roads_preset_splitter_2x2" => app.create_preset("splitter_2x2"),
        "roads_export" => {
            let ci = app
                .sel_container
                .or(app.sel.map(|(ci, _)| ci))
                .ok_or("Export needs a RoadContainer — select one in Layers or the plan first")?;
            app.export_container_gltf(ci)?;
        }
        "roads_make_lanes_editable" => app.make_lanes_editable(),
        "roads_report_issue" => {
            app.scene.borrow_mut().log(
                "[help] found a bug? Report it like the GRG plugin asks — https://github.com/TheDuckCow/godot-road-generator/issues",
            );
            app.status = "issue URL logged to the console".to_string();
        }
        "roads_share_feedback" => {
            app.scene.borrow_mut().log(
                "[help] share feedback — tell us what to build next on the godot-road-generator feedback thread",
            );
            app.status = "feedback note logged to the console".to_string();
        }
        /* ---------------- RoadPoint editor ---------------- */
        "point_add_fwd_lane" | "point_add_rev_lane" | "point_rem_fwd_lane" | "point_rem_rev_lane" => {
            let Some((ci, pi)) = app.sel else {
                return Err("select a RoadPoint first — lane buttons act on the selection".into());
            };
            let op = match id {
                "point_add_fwd_lane" => TrafficUpdate::AddForward,
                "point_add_rev_lane" => TrafficUpdate::AddReverse,
                "point_rem_fwd_lane" => TrafficUpdate::RemForward,
                _ => TrafficUpdate::RemReverse,
            };
            app.update_point_traffic(ci, pi, op, false)?;
        }
        "point_flip" => {
            let Some((ci, pi)) = app.sel else {
                return Err("select a RoadPoint first".into());
            };
            app.flip_point(ci, pi)?;
        }
        "point_copy_settings" => {
            let Some((ci, pi)) = app.sel else {
                return Err("select a RoadPoint first".into());
            };
            app.copy_point_settings(ci, pi)?;
        }
        "point_apply_settings" => {
            let Some((ci, pi)) = app.sel else {
                return Err("select a RoadPoint first".into());
            };
            app.apply_point_settings(ci, pi, false)?;
        }
        "point_apply_all" => {
            let Some((ci, pi)) = app.sel else {
                return Err("select a RoadPoint first".into());
            };
            app.apply_point_settings(ci, pi, true)?;
        }
        "point_select_next" => app.select_next_point(),
        "point_select_prior" => app.select_prior_point(),
        /* ---------------- History dock ---------------- */
        "history_undo" => app.history_undo(),
        "history_redo" => app.history_redo(),
        "history_clear" => app.history_clear(),
        /* ---------------- terrain + lane viz ---------------- */
        "terrain_toggle" => {
            let on = !app.scene.borrow().terrain.enabled;
            app.set_terrain_enabled(on);
        }
        "lanes_toggle_visibility" => {
            app.render.show_lanes = !app.render.show_lanes;
            app.render.dirty = true;
            let on = app.render.show_lanes;
            app.status = if on { "lanes visible" } else { "lanes hidden" }.to_string();
        }
        _ => return Err(format!("unknown action id '{id}' — call list_actions")),
    }
    if out.is_empty() {
        out = diff(&before, &snapshot(app));
    }
    Ok(out)
}

/// Compact one-line state summary for action diffs.
fn snapshot(app: &RoadkitApp) -> String {
    let sc = app.scene.borrow();
    format!(
        "tab={:?} tool={:?} curves={} points={} props={} junctions={} sel={:?} sel_c={:?} sel_j={:?} grid={} props={} spin={} sim={} autojoin={} round={} prefabs={} dock={:?} collapsed={} status='{}'",
        app.tab,
        app.tool,
        sc.curves.len(),
        sc.curves.iter().map(|c| c.points.len()).sum::<usize>(),
        sc.props.len(),
        sc.junctions.len(),
        app.sel,
        app.sel_container,
        app.sel_junction,
        app.render.show_grid,
        app.render.show_props,
        app.render.spin,
        app.render.sim_on,
        sc.junction_settings.enabled,
        sc.junction_settings.auto_roundabout,
        sc.junction_settings.use_prefabs,
        app.dock_tab,
        app.dock_collapsed,
        app.status,
    )
}

fn diff(a: &str, b: &str) -> String {
    if a == b {
        "state unchanged".to_string()
    } else {
        // show only the changed key=value fragments
        let pa: Vec<&str> = a.split(' ').collect();
        let pb: Vec<&str> = b.split(' ').collect();
        let changed: Vec<String> = pa
            .iter()
            .zip(pb.iter())
            .filter(|(x, y)| x != y)
            .map(|(x, y)| format!("{} → {}", x, y))
            .collect();
        changed.join(" · ")
    }
}
