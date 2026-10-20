//! Application state + script runner.

use crate::bridge::{self, SceneState, SharedScene};
use crate::history::{History, HistorySel};
use roads::model::TrafficUpdate;
#[allow(unused_imports)]
use crate::{panels, statusbar};
use crate::nodes_schema::NodeSchema;
use egui::TextureHandle;
use gdmath::vec2::Vec2;
use gdmath::OrbitCam;
use roadkit::{AssetLibrary, Scene, TextureLibrary};
use std::cell::RefCell;
use std::rc::Rc;
#[allow(unused_imports)]
use egui_kit::{Accent, Brightness};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tab {
    Scene3d,
    Plan,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tool {
    Select,
    AddPoint,
    Stamp,
    Pan,
}

impl Tool {
    pub fn label(&self) -> &'static str {
        match self {
            Tool::Select => "Select (V)",
            Tool::AddPoint => "Add RoadPoint (A)",
            Tool::Stamp => "Stamp GLB prop (P)",
            Tool::Pan => "Pan (H)",
        }
    }
    pub fn hotkey(&self) -> &'static str {
        match self {
            Tool::Select => "V",
            Tool::AddPoint => "A",
            Tool::Stamp => "P",
            Tool::Pan => "H",
        }
    }
}

#[derive(Clone, Debug)]
pub enum ScriptStatus {
    Idle,
    Ok(String),
    Err(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockTab {
    Layers,
    Properties,
    Signals,
    Section,
    Assets,
    Script,
    Console,
    History,
}

impl DockTab {
    pub const ALL: [DockTab; 8] = [
        DockTab::Layers,
        DockTab::Properties,
        DockTab::Signals,
        DockTab::Section,
        DockTab::Assets,
        DockTab::Script,
        DockTab::Console,
        DockTab::History,
    ];
    pub fn label(self) -> &'static str {
        match self {
            DockTab::Layers => "Layers",
            DockTab::Properties => "Properties",
            DockTab::Signals => "Signals",
            DockTab::Section => "Section",
            DockTab::Assets => "Assets",
            DockTab::Script => "Script",
            DockTab::Console => "Console",
            DockTab::History => "History",
        }
    }
    pub fn icon(self) -> egui_kit::IconName {
        match self {
            DockTab::Layers => egui_kit::IconName::Layers,
            DockTab::Properties => egui_kit::IconName::Sliders,
            // traffic light dot glyph — the kit's beacon/lamp icon if present,
            // else the console glyph reads as a head
            DockTab::Signals => egui_kit::IconName::Console,
            DockTab::Section => egui_kit::IconName::Sliders,
            DockTab::Assets => egui_kit::IconName::Box,
            DockTab::Script => egui_kit::IconName::Script,
            DockTab::Console => egui_kit::IconName::Console,
            // no clock glyph in the kit — the Console terminal reads as "history log"
            DockTab::History => egui_kit::IconName::Console,
        }
    }
    /// Tiny caption under the icon-column icon (readable controls).
    pub fn caption(self) -> &'static str {
        match self {
            DockTab::Layers => "Layer",
            DockTab::Properties => "Props",
            DockTab::Signals => "Signal",
            DockTab::Section => "Sect",
            DockTab::Assets => "Asset",
            DockTab::Script => "Code",
            DockTab::Console => "Log",
            DockTab::History => "Hist",
        }
    }
}

/// Which dock tabs are open (closable panels — the icon column toggles them).
#[derive(Clone, Copy, Debug)]
pub struct Panels {
    pub layers: bool,
    pub properties: bool,
    pub signals: bool,
    pub section: bool,
    pub assets: bool,
    pub script: bool,
    pub console: bool,
    pub history: bool,
}

impl Default for Panels {
    fn default() -> Self {
        // Terrazon parity: only Layers + Properties docked; Assets/Script/
        // Console/History live on the icon column until wanted.
        Self { layers: true, properties: true, signals: false, section: false, assets: false, script: false, console: false, history: false }
    }
}

impl Panels {
    /// Terrazon-style right panel: Layers + Properties visible, the rest
    /// reachable from the icon column (or popped out into windows).
    pub fn is_open(&self, tab: DockTab) -> bool {
        match tab {
            DockTab::Layers => self.layers,
            DockTab::Properties => self.properties,
            DockTab::Signals => self.signals,
            DockTab::Section => self.section,
            DockTab::Assets => self.assets,
            DockTab::Script => self.script,
            DockTab::Console => self.console,
            DockTab::History => self.history,
        }
    }
    pub fn set_open(&mut self, tab: DockTab, on: bool) {
        match tab {
            DockTab::Layers => self.layers = on,
            DockTab::Properties => self.properties = on,
            DockTab::Signals => self.signals = on,
            DockTab::Section => self.section = on,
            DockTab::Assets => self.assets = on,
            DockTab::Script => self.script = on,
            DockTab::Console => self.console = on,
            DockTab::History => self.history = on,
        }
    }
    pub fn open_tabs(&self) -> Vec<DockTab> {
        DockTab::ALL.iter().copied().filter(|t| self.is_open(*t)).collect()
    }
}

pub struct RenderState {
    pub scene3d: Option<Scene>,
    pub dirty: bool,
    pub tex: Option<TextureHandle>,
    pub show_grid: bool,
    pub show_props: bool,
    pub spin: bool,
    /// overlay the generated AI lanes (GRG draw_lanes) in 2D + 3D
    pub show_lanes: bool,
    /// overlay clickable RoadPoint markers (+ XYZ tripod on the selection)
    pub show_points: bool,
    /// per-intersection traffic simulation (vehicle agents + signals)
    pub sim: Option<roadkit::TrafficSim>,
    pub sim_on: bool,
    /// lane-graph simulation (simflow): per-lane alignment, turn
    /// connectors, red-light queues, zebra yields
    pub flow: Option<roadkit::flow::FlowSim>,
    pub flow_on: bool,
    /// Blender-style viewport shading (solid / normals / wireframe)
    pub shading: roadkit::raster::ShadeMode,
    pub sim_density: f64,
    /// left-hand traffic (Tanzania) — set when the DSM scenario loads
    pub sim_left_hand: bool,
    /// last CPU render duration (ms) — shown in the HUD
    pub render_ms: f32,
    /// signature of the last rendered frame (camera + size + toggles);
    /// when it matches and nothing is dirty the cached texture is reused
    pub last_sig: u64,
    /// a preset (DSM load / launch flag) already placed the camera —
    /// suppress the automatic hero framing on the first scene build
    pub cam_seeded: bool,
}

impl Default for RenderState {
    fn default() -> Self {
        Self {
            scene3d: None,
            dirty: true,
            tex: None,
            show_grid: true,
            show_props: true,
            spin: false,
            show_lanes: true,
            show_points: true,
            sim: None,
            sim_on: false,
            flow: None,
            flow_on: false,
            shading: roadkit::raster::ShadeMode::Solid,
            sim_density: 14.0,
            sim_left_hand: false,
            render_ms: 0.0,
            last_sig: 0,
            cam_seeded: false,
        }
    }
}

pub struct RoadkitApp {
    pub scene: SharedScene,
    pub render: RenderState,
    pub assets: AssetLibrary,
    /// road trim sheet + environment textures (assets/textures/*.raw)
    pub textures: TextureLibrary,
    pub cam: OrbitCam,
    /// node class + property registry loaded from assets/nodes.json
    pub schema: NodeSchema,

    pub tab: Tab,
    pub plan: PlanState,
    pub tool: Tool,
    pub stamp_asset: String,
    pub stamp_lateral: f64,
    /// selected (curve index, point index)
    pub sel: Option<(usize, usize)>,
    /// selected RoadContainer row (curve-level selection)
    pub sel_container: Option<usize>,
    /// selected junction in the Scene Tree / plan view
    pub sel_junction: Option<usize>,
    /// drag state for the plan view
    pub dragging: Option<DragKind>,

    /* ---- Intersection control registry (Signals dock) ---- */
    /// every controlled junction registers here (position-anchored)
    pub signals: roadkit::SignalRegistry,
    /// user nudges of traffic-light heads: (junction, group, +s, dx, dz)
    pub light_offsets: Vec<LightOffset>,
    /// selected signal head (index into sim.light_poses)
    pub sel_light: Option<usize>,
    /// 3D-viewport cursor ground hit (XYZ gizmo + status readout)
    pub cursor_world: Option<[f64; 3]>,
    /// the Dar es Salaam (JICA + OSM) scenario is loaded
    pub dsm_loaded: bool,

    pub panels: Panels,
    /// active dock tab (must be one of the open ones)
    pub dock_tab: DockTab,
    /// dock collapsed to the icon column
    pub dock_collapsed: bool,
    /// layer-tree expansion state (container names)
    pub expanded: std::collections::HashSet<String>,
    /// layer panel search filter
    pub layer_search: String,
    /// panels popped out of the dock into free-floating windows
    /// (Terrazon detachable panels) — insertion order = window order
    pub popped: Vec<DockTab>,
    /// Help ▸ "Road node structure" explainer window
    pub show_nodes_help: bool,
    /// UI look pickers
    pub brightness: Brightness,
    pub accent: Accent,
    pub script_idx: usize,
    pub script_src: String,
    pub script_status: ScriptStatus,
    pub console_seen: usize,

    pub fps: f32,
    pub last_t: f64,
    pub status: String,
    pub show_about: bool,
    pub show_api: bool,
    pub run_counter: u64,

    /* ---- Godot-style undo history (History dock) ---- */
    pub history: History,
    /// snapshot camera + selection with every history push
    pub history_scene: bool,
    pub history_global: bool,
    /// GRG "Copy Settings" clipboard (RoadPoint settings)
    pub point_clipboard: Option<roads::model::RoadPoint>,
}

/// GRG "Add next RoadPoint" spacing multiplier (lane_width × 8).
pub const SEG_DIST_MULT: f64 = 8.0;

/// A user nudge applied to one traffic-signal head (Signals panel +
/// 3D drag). Keyed by (junction, group, +s travel) so it survives sim
/// rebuilds; (dx, dz) is the offset from the auto-computed pole spot.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LightOffset {
    pub junction: usize,
    pub group: bool,
    pub positive_dir: bool,
    pub dx: f64,
    pub dz: f64,
}
/// Sanity cap for the lane buttons (GRG has no cap; the mesh does fine but
/// the trim sheet only has so many lane columns).
pub const MAX_LANES: usize = 12;

/// CLI launch options (headless testing / presets).
#[derive(Default, Clone)]
pub struct LaunchConfig {
    /// path to a .gd script to preload + run
    pub script: Option<String>,
    /// start tab override
    pub tab: Option<Tab>,
    /// active dock tab override (screenshots / presets)
    pub dock_tab: Option<DockTab>,
    /// load the Dar es Salaam (JICA + OSM) scenario instead of the demo
    pub dsm: bool,
    /// camera radius (m) for the DSM opening frame
    pub dsm_radius: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum DragKind {
    Pan { start_cx: f64, start_cz: f64 },
    Point { curve: usize, point: usize },
}

#[derive(Clone, Copy, Debug)]
pub struct PlanState {
    /// view center (meters)
    pub cx: f64,
    pub cz: f64,
    /// pixels per meter
    pub scale: f64,
}

impl Default for PlanState {
    fn default() -> Self {
        Self { cx: 0.0, cz: 0.0, scale: 1.6 }
    }
}

/// Search order for bundled assets: CWD, parent, exe-relative, env override.
pub fn find_assets_dir() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("ROADKIT_ASSETS") {
        let p = std::path::PathBuf::from(p);
        if p.is_dir() {
            return p;
        }
    }
    for cand in [
        std::path::PathBuf::from("assets"),
        std::path::PathBuf::from("../assets"),
        std::path::PathBuf::from("../../assets"),
        {
            let mut p = std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default();
            p.push("assets");
            p
        },
        {
            let mut p = std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.to_path_buf()))
                .unwrap_or_default();
            p.push("../assets");
            p
        },
    ] {
        if cand.join("glb").is_dir() {
            return cand;
        }
    }
    std::path::PathBuf::from("assets")
}

/// GRG refusal rules for a lane op on one point (used by the UI enabled
/// states AND by `update_point_traffic`).
fn op_accepted(pt: &roads::model::RoadPoint, op: TrafficUpdate) -> bool {
    use roads::model::TrafficUpdate::*;
    match op {
        AddForward | AddReverse => pt.lanes < MAX_LANES,
        RemForward => pt.lanes > 1 && pt.fwd_lanes() > 0,
        RemReverse => pt.lanes > 1 && pt.rev_lanes > 0,
        MoveDividerLeft | MoveDividerRight => false,
    }
}

impl RoadkitApp {
    pub fn new(cc: &eframe::CreationContext) -> Self {
        Self::with_config(cc, LaunchConfig::default())
    }

    pub fn with_config(cc: &eframe::CreationContext, cfg: LaunchConfig) -> Self {
        crate::theme::apply(&cc.egui_ctx);
        Self::core(cfg)
    }

    /// Headless constructor (MCP server / --uitest) — no egui context needed.
    pub fn core(cfg: LaunchConfig) -> Self {
        let assets_dir = find_assets_dir();
        let schema = NodeSchema::load(&assets_dir);
        // browser (wasm32) builds have no filesystem: the props + textures
        // ship embedded in the wasm (roadkit::library::AssetLibrary::embedded)
        #[cfg(target_arch = "wasm32")]
        let mut assets = AssetLibrary::embedded();
        #[cfg(not(target_arch = "wasm32"))]
        let mut assets = AssetLibrary::load_dir(&assets_dir);
        if assets.entries.is_empty() {
            eprintln!("[roadkit] no GLB assets found under {:?}", assets_dir);
        } else {
            assets.render_thumbs(96);
            eprintln!(
                "[roadkit] loaded {} GLB assets from {:?}",
                assets.entries.len(),
                assets_dir
            );
        }

        let scene: SharedScene = Rc::new(RefCell::new(SceneState::default()));
        #[cfg(target_arch = "wasm32")]
        let textures = TextureLibrary::load_embedded();
        #[cfg(not(target_arch = "wasm32"))]
        let textures = TextureLibrary::load_dir(&assets_dir.join("textures"));
        let mut app = Self {
            scene,
            render: RenderState::default(),
            assets,
            textures,
            cam: OrbitCam::default(),
            schema,
            tab: Tab::Scene3d,
            plan: PlanState::default(),
            tool: Tool::Select,
            stamp_asset: "streetlight".to_string(),
            stamp_lateral: 0.0,
            sel: None,
            sel_container: None,
            sel_junction: None,
            dragging: None,
            signals: roadkit::SignalRegistry::default(),
            light_offsets: Vec::new(),
            sel_light: None,
            cursor_world: None,
            dsm_loaded: false,
            panels: Panels::default(),
            dock_tab: DockTab::Layers,
            dock_collapsed: false,
            expanded: std::collections::HashSet::new(),
            layer_search: String::new(),
            popped: Vec::new(),
            show_nodes_help: false,
            brightness: Brightness::MediumDark,
            accent: Accent::Coral,
            script_idx: 0,
            script_src: crate::scripts::default_source(),
            script_status: ScriptStatus::Idle,
            console_seen: 0,
            fps: 0.0,
            last_t: 0.0,
            status: "ready — press F5 to run the script".to_string(),
            show_about: false,
            show_api: false,
            run_counter: 0,
            history: History::new(),
            history_scene: true,
            history_global: true,
            point_clipboard: None,
        };
        // CLI overrides: preload a script / start tab (headless tests)
        if let Some(path) = cfg.script {
            match std::fs::read_to_string(&path) {
                Ok(src) => {
                    app.script_src = src;
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if let Some(i) = crate::scripts::all().iter().position(|(_, f, _)| *f == name) {
                        app.script_idx = i;
                    }
                    app.status = format!("preloaded script {path}");
                }
                Err(e) => eprintln!("[roadkit] cannot read script {path}: {e}"),
            }
        }
        if let Some(t) = cfg.tab {
            app.tab = t;
        }
        if let Some(p) = cfg.dock_tab {
            app.panels.set_open(p, true);
            app.dock_tab = p;
        }
        // First impression: build the demo scene so GLB props are already
        // joined along the road when the app opens.
        app.run_script();
        if cfg.dsm {
            app.load_dsm();
            // optional close-up radius override (site framing comes from
            // load_dsm — it aims at the post-recenter site position)
            if cfg.dsm_radius > 0.0 {
                app.cam.dist = cfg.dsm_radius;
            }
        }
        // seed the undo history with the freshly built demo scene (the
        // History::new() seed keeps its "Initial state" label)
        app.history.entries[0].scene = app.scene.borrow().clone();
        app
    }

    /* ---------------- scripting ---------------- */

    pub fn run_script(&mut self) {
        self.run_counter += 1;
        let src = self.script_src.clone();
        // fresh scene per run — predictable output
        self.scene.borrow_mut().clear();
        self.render.dirty = true;
        self.sel = None;

        let mut interp = gdscript::Interpreter::new();
        bridge::register(&mut interp, &self.scene);
        match gdscript::parse(&src) {
            Err(e) => {
                self.scene.borrow_mut().log(format!("[error] parse: {e}"));
                self.script_status = ScriptStatus::Err(format!("parse: {e}"));
            }
            Ok(script) => match interp.run(&script, "main") {
                Err(e) => {
                    self.scene.borrow_mut().log(format!("[error] runtime: {e}"));
                    self.script_status = ScriptStatus::Err(e);
                }
                Ok(_) => {
                    let lines = interp.take_output();
                    let n_curves;
                    let n_props;
                    let n_junctions;
                    {
                        let mut sc = self.scene.borrow_mut();
                        for l in lines {
                            sc.log(l);
                        }
                        sc.dirty = true;
                        n_curves = sc.curves.len();
                        n_props = sc.props.len();
                        n_junctions = sc.junctions.len();
                        sc.log(format!(
                            "[done] scene: {n_curves} curves, {n_junctions} junctions, {n_props} GLB props"
                        ));
                    }
                    self.script_status =
                        ScriptStatus::Ok(format!("{n_curves} curves · {n_junctions} junctions · {n_props} props"));
                    self.status = format!("script OK — {n_curves} curves, {n_junctions} junctions, {n_props} props");
                    self.fit_scene();
                }
            },
        }
        self.console_seen = 0; // force console autoscroll
    }

    pub fn reset_scene(&mut self) {
        self.scene.borrow_mut().clear();
        self.render.dirty = true;
        self.sel = None;
        self.sel_container = None;
        self.sel_junction = None;
        self.script_status = ScriptStatus::Idle;
        self.status = "scene cleared".to_string();
    }

    /// Remove every placed GLB prop (keeps roads).
    pub fn clear_props(&mut self) {
        let mut sc = self.scene.borrow_mut();
        let n = sc.props.len();
        sc.props.clear();
        sc.dirty = true;
        sc.log(format!("[props] cleared all {n} props"));
        drop(sc);
        self.render.dirty = true;
        self.status = format!("{n} props cleared");
    }

    /// The selection covers exactly one node (point, container, or junction).
    pub fn clear_selection(&mut self) {
        self.sel = None;
        self.sel_container = None;
        self.sel_junction = None;
    }

    /// Select a RoadPoint WITHOUT hijacking the dock: the tree stays where
    /// the user is (Layers, Properties, a popped-out window…) and they open
    /// Properties themselves when they want it.
    pub fn select_point(&mut self, ci: usize, pi: usize) {
        self.clear_selection();
        self.sel = Some((ci, pi));
    }

    /// Select a whole RoadContainer without switching dock tabs.
    pub fn select_container(&mut self, ci: usize) {
        self.clear_selection();
        self.sel_container = Some(ci);
    }

    /// Select a junction without switching dock tabs.
    pub fn select_junction(&mut self, ji: usize) {
        self.clear_selection();
        self.sel_junction = Some(ji);
    }

    /* ---- Terrazon detachable panels -------------------------------- */

    /// Detach a dock tab into a free-floating window.
    pub fn pop_out(&mut self, tab: DockTab) {
        if !self.popped.contains(&tab) {
            self.popped.push(tab);
            self.panels.set_open(tab, true);
            // keep the dock on a tab that is still docked
            if self.dock_tab == tab {
                let next = self
                    .panels
                    .open_tabs()
                    .into_iter()
                    .find(|t| !self.popped.contains(t))
                    .or_else(|| self.panels.open_tabs().first().copied());
                if let Some(t) = next {
                    self.dock_tab = t;
                }
            }
            self.scene.borrow_mut().log(format!("[panel] '{}' popped out — drag it anywhere, the window button docks it back", tab.label()));
        }
    }

    /// Dock a floating panel back into the right dock.
    pub fn dock_back(&mut self, tab: DockTab) {
        self.popped.retain(|t| *t != tab);
        self.panels.set_open(tab, true);
        self.dock_tab = tab;
        self.scene.borrow_mut().log(format!("[panel] '{}' docked back into the right panel", tab.label()));
    }

    pub fn is_popped(&self, tab: DockTab) -> bool {
        self.popped.contains(&tab)
    }

    /// Add a fresh 2-point RoadContainer near the plan view center.
    pub fn add_road(&mut self) {
        let cx = self.plan.cx;
        let cz = self.plan.cz;
        self.add_road_at(cx, cz);
    }

    /// Add a fresh 2-point RoadContainer centered on (x, z).
    pub fn add_road_at(&mut self, x: f64, z: f64) {
        self.run_counter += 1;
        let mut curve = roads::model::RoadCurve::new(format!("Road {}", self.run_counter));
        let mut a = roads::model::RoadPoint::new(x - 40.0, z);
        a.bearing = 0.0;
        a.prior_mag = 13.0;
        a.next_mag = 13.0;
        let mut b = roads::model::RoadPoint::new(x + 40.0, z);
        b.bearing = 0.0;
        b.prior_mag = 13.0;
        b.next_mag = 13.0;
        curve.add_point(a);
        curve.add_point(b);
        let ci;
        {
            let mut sc = self.scene.borrow_mut();
            sc.curves.push(curve);
            ci = sc.curves.len() - 1;
            sc.dirty = true;
            sc.recompute_junctions();
            let name = sc.curves[ci].name.clone();
            sc.log(format!("[edit] new container '{name}' — drag its points in the 2D plan"));
        }
        self.render.dirty = true;
        self.clear_selection();
        self.sel_container = Some(ci);
        self.dock_tab = DockTab::Properties;
        self.status = "new RoadContainer added".to_string();
    }

    /// Delete a whole RoadContainer (layer-tree trash on a container row).
    pub fn delete_container(&mut self, ci: usize) {
        let mut sc = self.scene.borrow_mut();
        if ci < sc.curves.len() {
            let name = sc.curves[ci].name.clone();
            sc.curves.remove(ci);
            sc.dirty = true;
            sc.recompute_junctions();
            sc.log(format!("[edit] deleted container '{name}'"));
        }
        drop(sc);
        self.render.dirty = true;
        self.clear_selection();
    }

    pub fn set_script(&mut self, idx: usize) {
        let list = crate::scripts::all();
        if let Some((_, _, src)) = list.get(idx) {
            self.script_idx = idx;
            self.script_src = src.to_string();
            self.script_status = ScriptStatus::Idle;
        }
    }

    /// Frame the 3D camera + plan view on the scene bounds.
    pub fn fit_scene(&mut self) {
        let sc = self.scene.borrow();
        let (mut min_x, mut min_z) = (f64::INFINITY, f64::INFINITY);
        let (mut max_x, mut max_z) = (f64::NEG_INFINITY, f64::NEG_INFINITY);
        for c in &sc.curves {
            for p in &c.points {
                min_x = min_x.min(p.x);
                min_z = min_z.min(p.z);
                max_x = max_x.max(p.x);
                max_z = max_z.max(p.z);
            }
        }
        for p in &sc.props {
            min_x = min_x.min(p.pos.x as f64);
            min_z = min_z.min(p.pos.z as f64);
            max_x = max_x.max(p.pos.x as f64);
            max_z = max_z.max(p.pos.z as f64);
        }
        drop(sc);
        if !min_x.is_finite() {
            return;
        }
        let cx = (min_x + max_x) / 2.0;
        let cz = (min_z + max_z) / 2.0;
        let span = ((max_x - min_x).max(max_z - min_z)).max(40.0);
        self.cam.target = gdmath::Vec3::ZERO;
        self.cam.dist = (span * 1.35).clamp(30.0, 20_000.0) as f32;
        self.cam.pitch = 0.52;
        self.cam.yaw = 0.65;
        self.plan.cx = cx;
        self.plan.cz = cz;
        // plan scale: fit ~90% of a 900px viewport
        self.plan.scale = (760.0 / span).clamp(0.02, 40.0);
    }

    /* ---------------- editing helpers ---------------- */

    /// Load the Dar es Salaam scenario: real OSM ground network (Sam
    /// Nujoma / Morogoro / Nelson Mandela / Nyerere / Shekilango / Igesa
    /// + locals + footways) with the JICA TR2 engineering overlay —
    /// Kijazi split flyover + BRT median, Tazara 540 m flyover,
    /// 1L·2TH·1R turn approaches, 120/130 s signal plans (left-hand
    /// traffic). Replaces the current scene.
    pub fn load_dsm(&mut self) {
        let dsm = roadkit::build_dsm();
        let n_curves = dsm.curves.len();
        let n_signals = dsm.registry.entries.len();
        let pois = dsm.pois.clone();
        {
            let mut sc = self.scene.borrow_mut();
            sc.curves = dsm.curves;
            sc.props.clear();
            // real city corridor (9 km across) — the terrain heightfield is
            // a demo-scale feature and would oversize on this bbox
            sc.terrain.enabled = false;
            sc.lanes.clear();
            sc.lanes_editable = false;
            sc.recompute_junctions();
        }
        self.render.cam_seeded = true;
        self.signals = dsm.registry;
        self.light_offsets.clear();
        self.sel_light = None;
        self.sel = None;
        self.sel_container = None;
        self.sel_junction = None;
        self.dsm_loaded = true;
        self.render.sim_left_hand = true; // Tanzania drives on the left
        self.render.sim = None;
        self.render.dirty = true;
        self.script_status = ScriptStatus::Ok(format!(
            "Dar es Salaam loaded — {n_curves} curves · {n_signals} controlled junctions (JICA TR2)"
        ));
        self.status = format!(
            "DSM: {} curves, {} signals — Kijazi {}, Tazara {}, Igesa {} (LHT)",
            n_curves,
            n_signals,
            pois.first().map(|p| p.0.as_str()).unwrap_or(""),
            pois.get(2).map(|p| p.0.as_str()).unwrap_or(""),
            pois.get(3).map(|p| p.0.as_str()).unwrap_or(""),
        );
        // frame the Kijazi interchange (POI 0). The render scene subtracts
        // the whole-corridor bbox center, so aim the camera at the NEGATED
        // center — that is where Kijazi lands in render space.
        let mut min_x = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        let sc = self.scene.borrow();
        for c in &sc.curves {
            for p in &c.points {
                min_x = min_x.min(p.x);
                max_x = max_x.max(p.x);
                min_z = min_z.min(p.z);
                max_z = max_z.max(p.z);
            }
        }
        drop(sc);
        let (cx, cz) = (
            (min_x + max_x) / 2.0,
            (min_z + max_z) / 2.0,
        );
        self.cam.target = gdmath::Vec3::new(-cx as f32, 0.0, -cz as f32);
        self.cam.dist = 620.0;
        self.cam.pitch = 0.62;
        self.cam.yaw = 0.65;
        self.plan.cx = 0.0;
        self.plan.cz = 0.0;
        self.plan.scale = 0.55;
    }

    /// World-space plan transform helpers.
    pub fn plan_to_screen(&self, p: Vec2, rect: &egui::Rect) -> egui::Pos2 {
        egui::Pos2::new(
            ((p.x - self.plan.cx) * self.plan.scale + rect.center().x as f64) as f32,
            ((p.y - self.plan.cz) * self.plan.scale + rect.center().y as f64) as f32,
        )
    }

    pub fn screen_to_plan(&self, p: egui::Pos2, rect: &egui::Rect) -> Vec2 {
        Vec2::new(
            (p.x as f64 - rect.center().x as f64) / self.plan.scale + self.plan.cx,
            (p.y as f64 - rect.center().y as f64) / self.plan.scale + self.plan.cz,
        )
    }

    /// Add a point at world pos (AddPoint tool). Appends to the selected
    /// curve if any, else creates a new curve. When the new position lands
    /// within the snap threshold of another curve's endpoint, it snaps
    /// there (Godot-style auto join) and the junction re-detects.
    pub fn add_point_at(&mut self, x: f64, z: f64) {
        let snap = self.scene.borrow().junction_settings.snap_m;
        let mut sc = self.scene.borrow_mut();
        // auto-join: snap the incoming point onto a nearby endpoint
        let mut snapped: Option<(String, f64, f64)> = None;
        if sc.junction_settings.enabled {
            for (i, c) in sc.curves.iter().enumerate() {
                if c.closed || c.points.is_empty() {
                    continue;
                }
                let skip = matches!(self.sel, Some((si, _)) if si == i);
                if skip {
                    continue;
                }
                for (is_last, p) in [(false, &c.points[0]), (true, c.points.last().unwrap())] {
                    let d = ((p.x - x).powi(2) + (p.z - z).powi(2)).sqrt();
                    if d <= snap {
                        snapped = Some((format!("{} ({})", c.name, if is_last { "end" } else { "start" }), p.x, p.z));
                        break;
                    }
                }
            }
        }
        let (x, z) = match &snapped {
            Some((_, sx, sz)) => (*sx, *sz),
            None => (x, z),
        };
        match self.sel {
            Some((ci, _)) if ci < sc.curves.len() => {
                let curve = &mut sc.curves[ci];
                let bearing = match curve.points.last() {
                    Some(prev) => (z - prev.z).atan2(x - prev.x),
                    None => 0.0,
                };
                let mut pt = roads::model::RoadPoint::new(x, z);
                pt.bearing = bearing;
                if let Some(last) = curve.points.last() {
                    pt.lanes = last.lanes;
                    pt.lane_width = last.lane_width;
                    pt.oneway = last.oneway;
                }
                // auto-tune magnitudes to a third of the distance
                if let Some(prev) = curve.points.last() {
                    let d = ((x - prev.x).powi(2) + (z - prev.z).powi(2)).sqrt() / 3.0;
                    pt.prior_mag = d.max(2.0);
                    pt.next_mag = d.max(2.0);
                    let last = curve.points.last_mut().unwrap();
                    last.next_mag = d.max(2.0);
                }
                curve.add_point(pt);
                let pi = curve.points.len() - 1;
                let name = curve.name.clone();
                sc.dirty = true;
                sc.recompute_junctions();
                if let Some((target, _, _)) = &snapped {
                    sc.log(format!(
                        "[auto-join] new point snapped to {target} — junction will detect"
                    ));
                }
                sc.log(format!("[edit] added point {pi} to '{name}' — build_mesh() to apply"));
                drop(sc);
                self.sel = Some((ci, pi));
            }
            _ => {
                self.run_counter += 1;
                let mut curve = roads::model::RoadCurve::new(format!("Path {}", self.run_counter));
                curve.add_point(roads::model::RoadPoint::new(x, z));
                let name = curve.name.clone();
                sc.curves.push(curve);
                let ci = sc.curves.len() - 1;
                sc.dirty = true;
                sc.recompute_junctions();
                sc.log(format!("[edit] new curve '{name}' — add more points, then build_mesh()"));
                drop(sc);
                self.sel = Some((ci, 0));
            }
        }
        self.render.dirty = true;
    }

    /// Auto-join pass after a point drag ends: if the dragged point is a
    /// curve ENDPOINT within snap distance of another curve's endpoint,
    /// weld them to the shared mean (GRG connection-tool behavior).
    pub fn weld_after_drag(&mut self, ci: usize, pi: usize) {
        let (is_endpoint, px, pz) = {
            let sc = self.scene.borrow();
            let Some(c) = sc.curves.get(ci) else { return };
            if c.points.is_empty() || !(pi == 0 || pi == c.points.len() - 1) {
                return;
            }
            (true, c.points[pi].x, c.points[pi].z)
        };
        if !is_endpoint {
            return;
        }
        let snap = self.scene.borrow().junction_settings.snap_m;
        // find the nearest OTHER curve endpoint
        let mut best: Option<(f64, usize, usize)> = None;
        {
            let sc = self.scene.borrow();
            for (i, c) in sc.curves.iter().enumerate() {
                if i == ci || c.closed || c.points.is_empty() {
                    continue;
                }
                for (pi2, p) in [(0usize, &c.points[0]), (c.points.len() - 1, c.points.last().unwrap())] {
                    let d = ((p.x - px).powi(2) + (p.z - pz).powi(2)).sqrt();
                    if d <= snap && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                        best = Some((d, i, pi2));
                    }
                }
            }
        }
        let Some((d, ci2, pi2)) = best else { return };
        let mut sc = self.scene.borrow_mut();
        let mx = (sc.curves[ci].points[pi].x + sc.curves[ci2].points[pi2].x) / 2.0;
        let mz = (sc.curves[ci].points[pi].z + sc.curves[ci2].points[pi2].z) / 2.0;
        sc.curves[ci].points[pi].x = mx;
        sc.curves[ci].points[pi].z = mz;
        sc.curves[ci2].points[pi2].x = mx;
        sc.curves[ci2].points[pi2].z = mz;
        sc.curves[ci].auto_orient();
        sc.curves[ci2].auto_orient();
        sc.recompute_junctions();
        let n = sc.junctions.len();
        let name2 = sc.curves[ci2].name.clone();
        sc.log(format!(
            "[auto-join] welded endpoints {d:.1} m apart onto '{name2}' — {n} junction(s) in scene"
        ));
        sc.dirty = true;
        drop(sc);
        self.render.dirty = true;
    }

    /// Stamp the selected asset at the given world position on the nearest
    /// curve (Stamp tool).
    pub fn stamp_at(&mut self, x: f64, z: f64) {
        let asset = self.stamp_asset.clone();
        let lateral = self.stamp_lateral;

        // find the nearest curve to the click
        let mut best: Option<(f64, usize, f64)> = None; // (dist, curve idx, s)
        {
            let sc = self.scene.borrow();
            for (i, c) in sc.curves.iter().enumerate() {
                if let Some((s, d)) = c.closest_s(x, z) {
                    if best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                        best = Some((d, i, s));
                    }
                }
            }
        }

        let Some((d, ci, s)) = best else {
            self.scene
                .borrow_mut()
                .log("[edit] no road found — build one first (run the script or use Add RoadPoint)");
            return;
        };
        if d > 40.0 {
            self.scene.borrow_mut().log(format!(
                "[edit] too far from any road ({d:.0} m) — click on a road to stamp"
            ));
            return;
        }

        // compute the prop while the borrow is alive
        let placed: Option<(roadkit::PropInstance, String, f64)> = {
            let sc = self.scene.borrow();
            let curve = &sc.curves[ci];
            let length = curve.length_m();
            if length <= 0.0 {
                None
            } else {
                let t = (s / length).clamp(0.0, 1.0);
                roadkit::stamp(curve, &asset, t, lateral, true, 1.0)
                    .map(|p| (p, curve.name.clone(), t))
            }
        };

        if let Some((prop, cname, t)) = placed {
            let mut sc = self.scene.borrow_mut();
            sc.props.push(prop);
            sc.dirty = true;
            sc.log(format!(
                "[props] stamped {asset} on '{cname}' at {t:.2} (lateral {lateral:.1} m)"
            ));
            drop(sc);
            self.render.dirty = true;
        }
    }

    /// Delete the selected road point.
    pub fn delete_selected_point(&mut self) {
        let Some((ci, pi)) = self.sel else { return };
        let mut sc = self.scene.borrow_mut();
        if ci < sc.curves.len() && pi < sc.curves[ci].points.len() {
            let name = sc.curves[ci].name.clone();
            sc.curves[ci].points.remove(pi);
            sc.dirty = true;
            sc.log(format!("[edit] removed point {pi} from '{name}'"));
        }
        drop(sc);
        self.sel = None;
        self.render.dirty = true;
    }

    /* ---------------- Godot-style undo history ---------------- */

    /// Commit a history entry: full scene snapshot + (when `history_global`)
    /// the camera and selection. The scene is CLONED FIRST so the borrow of
    /// `self.scene` never overlaps the `&mut self` of the caller's mutation.
    pub fn push_history(&mut self, label: impl Into<String>) {
        let label = label.into();
        let scene = self.scene.borrow().clone();
        let global = self.history_global;
        let cam = if global {
            Some([
                self.cam.yaw as f64,
                self.cam.pitch as f64,
                self.cam.dist as f64,
                self.cam.target.x as f64,
                self.cam.target.z as f64,
            ])
        } else {
            None
        };
        let sel: Option<HistorySel> = if global {
            Some((self.sel, self.sel_container, self.sel_junction))
        } else {
            None
        };
        self.history.push(label, scene, cam, sel);
    }

    /// Restore any entry (History dock row click / undo / redo): writes the
    /// snapshot back into the scene, re-detects junctions + lanes and
    /// restores camera/selection when the global snapshot is present.
    pub fn restore_history(&mut self, idx: usize) {
        let Some(entry) = self.history.entries.get(idx) else { return };
        let (label, scene, cam, sel) = (
            entry.label.clone(),
            entry.scene.clone(),
            entry.cam,
            entry.sel,
        );
        let old_cursor = self.history.cursor;
        // Godot names the ACTION in the status bar: undoing from entry k
        // undoes entries[k]'s action; redoing to entry j redoes its action.
        let left_label = self
            .history
            .entries
            .get(old_cursor)
            .map(|e| e.label.clone())
            .unwrap_or_default();
        self.history.cursor = idx;
        {
            let mut sc = self.scene.borrow_mut();
            *sc = scene;
            sc.dirty = true;
            sc.recompute_junctions();
        }
        self.render.dirty = true;
        if let (true, Some([yaw, pitch, dist, tx, tz])) = (self.history_global, cam) {
            self.cam.yaw = yaw as f32;
            self.cam.pitch = pitch as f32;
            self.cam.dist = dist as f32;
            self.cam.target = gdmath::Vec3::new(tx as f32, 0.0, tz as f32);
            self.plan.cx = tx;
            self.plan.cz = tz;
        }
        if let (true, Some((sel_pt, sel_c, sel_j))) = (self.history_global, sel) {
            self.sel = sel_pt;
            self.sel_container = sel_c;
            self.sel_junction = sel_j;
        }
        self.status = if idx < old_cursor {
            format!("Undid: {left_label}")
        } else if idx > old_cursor {
            format!("Redid: {label}")
        } else {
            format!("History → '{label}'")
        };
    }

    pub fn history_undo(&mut self) {
        if self.history.cursor == 0 {
            self.status = "nothing to undo".to_string();
            return;
        }
        self.restore_history(self.history.cursor - 1);
    }

    pub fn history_redo(&mut self) {
        if self.history.cursor + 1 >= self.history.entries.len() {
            self.status = "nothing to redo".to_string();
            return;
        }
        self.restore_history(self.history.cursor + 1);
    }

    /// Drop the whole stack, keeping the live state as the new "Initial
    /// state" entry.
    pub fn history_clear(&mut self) {
        let cur = self.history.cursor;
        let mut kept = vec![self.history.entries[cur].clone()];
        kept[0].label = "Initial state".to_string();
        self.history.entries = kept;
        self.history.cursor = 0;
        self.scene
            .borrow_mut()
            .log("[history] undo stack cleared — the current state is kept");
        self.status = "history cleared".to_string();
    }

    /* ---------------- GRG Roads-menu helpers ---------------- */

    /// GRG "Add next RoadPoint": appends a point continuing from the
    /// selected one (spacing = lane_width × SEG_DIST_MULT, settings copied
    /// from it), or starts a fresh container when nothing is selected.
    pub fn add_point_grg(&mut self, x: f64, z: f64) {
        let sel = self.sel;
        if let Some((ci, pi)) = sel {
            let valid = {
                let sc = self.scene.borrow();
                sc.curves.get(ci).map(|c| pi < c.points.len()).unwrap_or(false)
            };
            if valid {
                let (nx, nz) = {
                    let sc = self.scene.borrow();
                    let src = &sc.curves[ci].points[pi];
                    let (dx, dz) = src.dir();
                    let dist = SEG_DIST_MULT * src.lane_width;
                    (src.x + dx * dist, src.z + dz * dist)
                };
                let mut pt = {
                    let sc = self.scene.borrow();
                    let src = sc.curves[ci].points[pi].clone();
                    let mut np = roads::model::RoadPoint::new(nx, nz);
                    np.copy_settings_from(&src); // lanes / rev_lanes / median / dirt / …
                    np.prior_mag = (src.lane_width * src.lanes as f64).max(2.0);
                    np.next_mag = np.prior_mag;
                    np.auto_bearing = true;
                    np
                };
                pt.name = String::new();
                {
                    let mut sc = self.scene.borrow_mut();
                    sc.curves[ci].points.insert(pi + 1, pt);
                    sc.curves[ci].auto_orient();
                    sc.dirty = true;
                    sc.recompute_junctions();
                    let name = sc.curves[ci].name.clone();
                    sc.log(format!("[edit] added the next RoadPoint to '{name}' — {SEG_DIST_MULT:.0}× lane_width away"));
                }
                self.render.dirty = true;
                self.sel = Some((ci, pi + 1));
                self.sel_container = Some(ci);
                self.sel_junction = None;
                self.dock_tab = DockTab::Properties;
                self.push_history("Add next RoadPoint");
                self.status = "next RoadPoint added".to_string();
                return;
            }
        }
        // nothing selected → GRG starts a fresh container here
        self.add_road_at(x, z);
        self.push_history("Add RoadContainer");
    }

    /// GRG "Add 2x2 road segment": two points 32 m apart, 2 lanes each way
    /// (traffic_dir R,R,F,F), lane_width 4, handles sized lane_width×lanes.
    pub fn create_2x2_road(&mut self) {
        self.run_counter += 1;
        let cx = self.plan.cx;
        let cz = self.plan.cz;
        let mut curve = roads::model::RoadCurve::new(format!("Road {}", self.run_counter));
        let mut a = roads::model::RoadPoint::new(cx - 16.0, cz);
        a.bearing = 0.0;
        a.lanes = 4;
        a.rev_lanes = 2;
        a.lane_width = 4.0;
        a.prior_mag = 16.0;
        a.next_mag = 16.0;
        a.auto_bearing = true;
        let mut b = roads::model::RoadPoint::new(cx + 16.0, cz);
        b.bearing = 0.0;
        b.lanes = 4;
        b.rev_lanes = 2;
        b.lane_width = 4.0;
        b.prior_mag = 16.0;
        b.next_mag = 16.0;
        b.auto_bearing = true;
        curve.add_point(a);
        curve.add_point(b);
        let ci;
        {
            let mut sc = self.scene.borrow_mut();
            sc.curves.push(curve);
            ci = sc.curves.len() - 1;
            sc.dirty = true;
            sc.recompute_junctions();
            let name = sc.curves[ci].name.clone();
            sc.log(format!(
                "[edit] added 2x2 road segment '{name}' — 2 lanes each way, 32 m long"
            ));
        }
        self.render.dirty = true;
        self.clear_selection();
        self.sel_container = Some(ci);
        self.sel = Some((ci, 1));
        self.dock_tab = DockTab::Properties;
        self.push_history("Add 2x2 road segment");
        self.status = "2x2 road segment added".to_string();
    }

    /// GRG "RoadContainer presets": spawn N arms meeting at the plan center
    /// (lanes from the id suffix) and pin the matching prefab tile onto the
    /// junction that forms there.
    pub fn create_preset(&mut self, id: &str) {
        // lanes per arm from the id suffix: "1x1" → 2 lanes (1 rev), "2x2" → 4 (2 rev)
        let (lanes, rev) = if id.ends_with("2x2") { (4usize, 2usize) } else { (2, 1) };
        let mag = (4.0 * lanes as f64).clamp(2.0, 16.0);
        let roundabout = id.contains("roundabout");
        let arms: Vec<(f64, f64)> = if id.contains("4way") || roundabout {
            vec![(0.0, 40.0), (90.0, 40.0), (180.0, 40.0), (270.0, 40.0)]
        } else if id.contains("3way") {
            // 3 arms at 120° spacing, first arm east
            vec![(0.0, 40.0), (120.0, 40.0), (240.0, 40.0)]
        } else if id.contains("splitter") {
            // 4 arms, the merge leg shorter
            vec![(90.0, 40.0), (248.0, 40.0), (292.0, 40.0), (0.0, 24.0)]
        } else {
            // highway on/off-ramps: 2 arms at 45°
            vec![(0.0, 40.0), (45.0, 40.0)]
        };
        let cx = self.plan.cx;
        let cz = self.plan.cz;
        let mut new_curves = Vec::new();
        for &(deg, len) in &arms {
            self.run_counter += 1;
            let mut curve = roads::model::RoadCurve::new(format!("Arm {}", self.run_counter));
            let a = deg.to_radians();
            let mut p0 = roads::model::RoadPoint::new(cx, cz);
            let mut p1 =
                roads::model::RoadPoint::new(cx + a.cos() * len, cz + a.sin() * len);
            for p in [&mut p0, &mut p1] {
                p.lanes = lanes;
                p.rev_lanes = rev;
                p.lane_width = 4.0;
                p.prior_mag = mag;
                p.next_mag = mag;
                p.auto_bearing = true;
            }
            curve.add_point(p0);
            curve.add_point(p1);
            new_curves.push(curve);
        }
        let key = (cx.round() as i64, cz.round() as i64);
        let n_arms = arms.len();
        {
            let mut sc = self.scene.borrow_mut();
            sc.curves.extend(new_curves);
            sc.dirty = true;
            // force the matching prefab tile at the center junction
            sc.prefab_override.insert(key, id.to_string());
            if roundabout {
                sc.junction_override.insert(key, roads::JunctionKind::Roundabout);
            } else if n_arms >= 3 {
                sc.junction_override.insert(key, roads::JunctionKind::Crossing);
            }
            sc.recompute_junctions();
            sc.log(format!(
                "[presets] spawned {n_arms}-arm RoadScene '{id}' at ({cx:.0}, {cz:.0})"
            ));
        }
        self.render.dirty = true;
        let ji = self
            .scene
            .borrow()
            .junctions
            .iter()
            .position(|j| (j.x - cx).hypot(j.z - cz) < 5.0);
        self.clear_selection();
        if let Some(ji) = ji {
            self.sel_junction = Some(ji);
            self.dock_tab = DockTab::Properties;
        }
        self.push_history(format!("Add RoadScene ({id})"));
        self.status = format!("preset '{id}' added");
    }

    /// GRG "Add RoadLane": regenerate the AI lane graph for the selected
    /// container (every container when none is selected) and make lanes
    /// editable.
    pub fn add_lane_node(&mut self) {
        let ci = self.sel_container.or(self.sel.map(|(ci, _)| ci));
        let n_lanes;
        {
            let mut sc = self.scene.borrow_mut();
            match ci {
                Some(ci) if ci < sc.curves.len() => {
                    // one container: enable lanes + editability on it
                    sc.curves[ci].generate_ai_lanes = true;
                    sc.lanes_editable = true;
                }
                _ => {
                    // nothing selected → the whole manager (GRG adds the lane
                    // under RoadManager when the selection is the manager)
                    for c in &mut sc.curves {
                        c.generate_ai_lanes = true;
                    }
                    sc.lanes_editable = true;
                }
            }
            sc.recompute_junctions();
            n_lanes = sc.lanes.len();
            sc.dirty = true;
            sc.log(format!("[lanes] RoadLane added — {n_lanes} lanes in the scene"));
        }
        self.render.dirty = true;
        self.push_history("Add RoadLane");
        self.status = format!("RoadLane added — {n_lanes} lanes generated");
    }

    /// GRG "Add RoadLaneAgent": registers an AI agent and switches the
    /// traffic simulation on so it can be watched driving the lanes.
    pub fn add_lane_agent(&mut self) {
        self.render.sim_on = true;
        self.render.dirty = true;
        self.scene.borrow_mut().log(
            "[ai] RoadLaneAgent registered — enable the traffic toggle to watch it drive the lanes",
        );
        self.push_history("Add RoadLaneAgent");
        self.status = "RoadLaneAgent registered".to_string();
    }

    /// GRG "Add RoadTerrain3DConnector": hills + flatten-under-roads on.
    pub fn add_terrain_connector(&mut self) {
        {
            let mut sc = self.scene.borrow_mut();
            sc.terrain.enabled = true;
            // terrain has no junction effect, but recompute keeps parity with
            // every other scene-level mutation
            sc.recompute_junctions();
            sc.log(
                "[terrain] RoadTerrain3DConnector added — hills enabled, roads flatten the ground",
            );
        }
        self.render.dirty = true;
        self.push_history("Add RoadTerrain3DConnector");
        self.status = "RoadTerrain3DConnector added".to_string();
    }

    /// The RoadManager "Set terrain" toggle (history-named like Godot).
    pub fn set_terrain_enabled(&mut self, on: bool) {
        {
            let mut sc = self.scene.borrow_mut();
            if sc.terrain.enabled == on {
                return;
            }
            sc.terrain.enabled = on;
            sc.recompute_junctions();
            sc.log(format!(
                "[terrain] connector {} — ground {}",
                if on { "enabled" } else { "disabled" },
                if on { "follows the hills" } else { "flat again" }
            ));
        }
        self.render.dirty = true;
        self.push_history("Set terrain");
        self.status = if on { "terrain enabled" } else { "terrain disabled" }.to_string();
    }

    /// GRG lane add/remove on a RoadPoint (the `+ - || - +` row). `bulk`
    /// (shift-click / "apply to whole container") applies the same op to
    /// every point of the container. Refuses with a clear message when the
    /// op is a no-op (GRG rules: a road keeps >= 1 lane, an empty side
    /// cannot shrink).
    pub fn update_point_traffic(
        &mut self,
        ci: usize,
        pi: usize,
        op: TrafficUpdate,
        bulk: bool,
    ) -> Result<(), String> {
        let targets: Vec<usize> = {
            let sc = self.scene.borrow();
            let Some(curve) = sc.curves.get(ci) else {
                return Err("RoadPoint selection is out of range — scene changed?".into());
            };
            if pi >= curve.points.len() {
                return Err("RoadPoint selection is out of range — scene changed?".into());
            }
            if bulk {
                (0..curve.points.len()).collect()
            } else {
                vec![pi]
            }
        };
        // pre-check refusals (all-or-nothing for bulk)
        {
            let sc = self.scene.borrow();
            let cname = sc.curves[ci].name.clone();
            for i in &targets {
                let pt = &sc.curves[ci].points[*i];
                if !op_accepted(pt, op) {
                    let reason = match op {
                        TrafficUpdate::RemForward | TrafficUpdate::RemReverse => {
                            let side = if matches!(op, TrafficUpdate::RemReverse) {
                                "reverse"
                            } else {
                                "forward"
                            };
                            format!(
                                "cannot remove the {side} lane — a road keeps at least 1 lane and a non-empty side (GRG rule)"
                            )
                        }
                        TrafficUpdate::AddForward | TrafficUpdate::AddReverse => format!(
                            "cannot add another lane — {MAX_LANES} lanes max (trim sheet limit)"
                        ),
                        _ => "lane operation refused".into(),
                    };
                    return Err(format!("{reason} [\"{cname}\"]"));
                }
            }
        }
        {
            let mut sc = self.scene.borrow_mut();
            for i in &targets {
                sc.curves[ci].points[*i].update_traffic_dir(op);
            }
            sc.dirty = true;
            sc.recompute_junctions();
        }
        self.render.dirty = true;
        self.push_history(op.undo_label());
        self.status = op.undo_label().to_string();
        Ok(())
    }

    /// GRG "Flip RoadPoint" (180° re-orientation, mags/shoulders swapped).
    pub fn flip_point(&mut self, ci: usize, pi: usize) -> Result<(), String> {
        {
            let mut sc = self.scene.borrow_mut();
            let Some(pt) = sc.curves.get_mut(ci).and_then(|c| c.points.get_mut(pi)) else {
                return Err("RoadPoint selection is out of range — scene changed?".into());
            };
            pt.flip();
            sc.dirty = true;
            sc.recompute_junctions();
        }
        self.render.dirty = true;
        self.push_history("Flip RoadPoint");
        self.status = "RoadPoint flipped".to_string();
        Ok(())
    }

    /// GRG "Copy Settings": stash a RoadPoint's cross-section settings.
    pub fn copy_point_settings(&mut self, ci: usize, pi: usize) -> Result<(), String> {
        let pt = {
            let sc = self.scene.borrow();
            sc.curves
                .get(ci)
                .and_then(|c| c.points.get(pi))
                .cloned()
                .ok_or("RoadPoint selection is out of range — scene changed?")?
        };
        self.point_clipboard = Some(pt);
        self.scene
            .borrow_mut()
            .log("[edit] RoadPoint settings copied — select another point and Apply");
        self.status = "RoadPoint settings copied".to_string();
        Ok(())
    }

    /// GRG "Apply Settings": paste the clipboard onto one RoadPoint (or
    /// every point of its container when `all`).
    pub fn apply_point_settings(&mut self, ci: usize, pi: usize, all: bool) -> Result<(), String> {
        let Some(clip) = self.point_clipboard.clone() else {
            return Err("no copied settings — use Copy Settings on a RoadPoint first".into());
        };
        {
            let mut sc = self.scene.borrow_mut();
            let Some(curve) = sc.curves.get_mut(ci) else {
                return Err("RoadPoint selection is out of range — scene changed?".into());
            };
            if all {
                for p in &mut curve.points {
                    p.copy_settings_from(&clip);
                }
            } else {
                let Some(p) = curve.points.get_mut(pi) else {
                    return Err("RoadPoint selection is out of range — scene changed?".into());
                };
                p.copy_settings_from(&clip);
            }
            sc.dirty = true;
            sc.recompute_junctions();
            let cname = sc.curves[ci].name.clone();
            sc.log(format!(
                "[edit] applied copied settings to {} of '{cname}'",
                if all { "ALL points" } else { "one point" }
            ));
        }
        self.render.dirty = true;
        self.push_history(if all {
            "Apply settings to all container RoadPoints"
        } else {
            "Apply settings to RoadPoint"
        });
        self.status = "RoadPoint settings applied".to_string();
        Ok(())
    }

    /// Godot "Select Next RoadPoint" — walk the selection along the tree.
    pub fn select_next_point(&mut self) {
        self.step_point_selection(1);
    }

    /// Godot "Select Prior RoadPoint".
    pub fn select_prior_point(&mut self) {
        self.step_point_selection(-1);
    }

    fn step_point_selection(&mut self, delta: i64) {
        // Godot walks the selection ALONG THE CURVE, clamping at the ends.
        // With nothing selected it grabs point 0 of the first container.
        let counts: Vec<usize> = self
            .scene
            .borrow()
            .curves
            .iter()
            .map(|c| c.points.len())
            .collect();
        if counts.is_empty() || counts.iter().all(|n| *n == 0) {
            self.status = "no RoadPoints to select — add a road first".to_string();
            return;
        }
        let (ci, pi) = match self.sel {
            Some((ci, pi)) if ci < counts.len() && pi < counts[ci] => {
                let next = (pi as i64 + delta).clamp(0, counts[ci] as i64 - 1) as usize;
                (ci, next)
            }
            _ => {
                // none selected (or stale) → point 0 of the first container
                let ci = counts.iter().position(|n| *n > 0).unwrap_or(0);
                (ci, 0)
            }
        };
        self.sel = Some((ci, pi));
        self.sel_container = Some(ci);
        self.sel_junction = None;
        let cname = self.scene.borrow().curves[ci].name.clone();
        self.status = format!(
            "selected RoadPoint {} of '{cname}' ({})",
            pi + 1,
            if delta > 0 { "next" } else { "prior" }
        );
    }

    /// GRG "Export RoadContainer": write the container's generated geometry
    /// (flat ribbon at road surface) to dist/exports/<name>_geo.glb.
    pub fn export_container_gltf(&mut self, ci: usize) -> Result<(), String> {
        let curves = {
            let sc = self.scene.borrow();
            match sc.curves.get(ci) {
                Some(c) => vec![c.clone()],
                None => {
                    return Err("RoadContainer selection is out of range — scene changed?".into())
                }
            }
        };
        let name = curves[0].name.clone();
        let safe: String = name
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '_' })
            .collect();
        let dir = std::path::Path::new("dist/exports");
        let path = dir.join(format!("{safe}_geo.glb"));
        let tris = roadkit::export::export_roads_glb(&curves, &path)?;
        let shown = path.display().to_string();
        self.scene.borrow_mut().log(format!(
            "[export] wrote {shown} ({tris} tris) — like GRG Export RoadContainer"
        ));
        self.status = format!("exported {tris} tris → {shown}");
        Ok(())
    }

    /// GRG "Make RoadLanes Editable": promote the generated lane graph to
    /// editable lanes and turn on the editor visualization everywhere.
    pub fn make_lanes_editable(&mut self) {
        {
            let mut sc = self.scene.borrow_mut();
            sc.lanes_editable = true;
            for c in &mut sc.curves {
                c.draw_lanes_editor = true;
            }
            sc.recompute_junctions();
            let n = sc.lanes.len();
            sc.log(format!(
                "[lanes] made {n} RoadLanes editable — draw_lanes_editor on for every container"
            ));
        }
        self.render.dirty = true;
        self.push_history("Make RoadLanes Editable");
        self.status = "RoadLanes made editable".to_string();
    }
}

impl eframe::App for RoadkitApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // fps
        let now = ctx.input(|i| i.time);
        let dt = (now - self.last_t) as f32;
        if dt > 0.0 && dt < 5.0 {
            self.fps = if self.fps <= 0.0 { 1.0 / dt } else { self.fps * 0.9 + (1.0 / dt) * 0.1 };
        }
        self.last_t = now;

        // smooth zoom (Google-Maps feel): consume + decay the wheel velocity
        if self.cam.zoom_vel.abs() > 0.01 {
            let k = (self.cam.zoom_vel * 0.16).clamp(-self.cam.dist * 0.28, self.cam.dist * 0.28);
            self.cam.dist = (self.cam.dist + k).clamp(4.0, 60_000.0);
            self.cam.zoom_vel *= 0.82;
            self.render.dirty = true;
            ctx.request_repaint();
        }

        // global shortcuts
        if ctx.input(|i| i.key_pressed(egui::Key::F5)) {
            self.run_script();
        }
        if ctx.input(|i| i.key_pressed(egui::Key::F) && !i.modifiers.any()) {
            self.fit_scene();
        }

        // VectorCraft-style frame order: bars → right dock → central doc tab
        // + view → status/hint → floating windows → dialogs. (The old left
        // vertical tool rail is gone — Terrazon keeps its tools in the top
        // control bar.)
        crate::chrome::app_bar(self, ctx);
        crate::chrome::control_bar(self, ctx);
        crate::panels::icon_column(self, ctx);
        crate::panels::dock(self, ctx);
        crate::chrome::central(self, ctx);
        crate::statusbar::status_bar(self, ctx);
        crate::statusbar::hint_bar(self, ctx);
        crate::panels::floating_panels(self, ctx);
        crate::panels::dialogs(self, ctx);

        if self.render.spin {
            ctx.request_repaint();
        }
    }
}


impl RoadkitApp {
    /// The "Join GLB props like Godot" bulk action (GRG-style dressing):
    /// vehicles + streetlights + trees + milestones joined along every road.
    pub fn join_props_like_godot(&mut self) {
        use roadkit::JoinParams;
        let curves = self.scene.borrow().curves.clone();
        let mut out: Vec<roadkit::PropInstance> = Vec::new();
        for c in &curves {
            if c.points.len() < 2 {
                continue;
            }
            let lat = c.paved_width() / 2.0;
            let seed = self.run_counter.wrapping_mul(2654435761).wrapping_add(c.name.len() as u64);
            let vehicle = JoinParams {
                spacing: 90.0,
                lateral: lat * 0.5,
                alternate: true,
                align: true,
                scale: 1.0,
                jitter: 0.25,
                random_yaw: false,
                seed: seed + 1,
            };
            out.extend(roadkit::join_along(c, "vehicle", &vehicle));

            let street = JoinParams {
                spacing: 32.0,
                lateral: lat + 2.0,
                alternate: true,
                align: true,
                scale: 1.0,
                jitter: 0.0,
                random_yaw: false,
                seed: seed + 2,
            };
            out.extend(roadkit::join_along(c, "streetlight", &street));

            let trees = JoinParams {
                spacing: 17.0,
                lateral: lat + 5.0,
                alternate: true,
                align: false,
                scale: 1.0,
                jitter: 0.35,
                random_yaw: true,
                seed: seed + 3,
            };
            out.extend(roadkit::join_along(c, "tree", &trees));

            let miles = JoinParams {
                spacing: 120.0,
                lateral: 3.5,
                alternate: false,
                align: true,
                scale: 1.0,
                jitter: 0.0,
                random_yaw: false,
                seed: seed + 4,
            };
            out.extend(roadkit::join_along(c, "milestone", &miles));
        }
        let added = out.len();
        let mut sc = self.scene.borrow_mut();
        sc.props.extend(out);
        sc.dirty = true;
        let curves_n = sc.curves.len();
        sc.log(format!("[props] joined {added} GLB props across {curves_n} curves (GRG-style dressing)"));
        drop(sc);
        self.render.dirty = true;
        self.status = format!("{added} GLB props joined like Godot");
    }
}
