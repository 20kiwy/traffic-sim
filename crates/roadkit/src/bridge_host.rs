//! The script bridge: `SceneState` + the Godot objects that .gd scripts
//! talk to (`RoadPoint`, `RoadCurve`) + constructor registration.
//!
//! This is the Godot-Road-Generator workflow rendered in Rust: a script
//! instantiates RoadPoints, sets transforms/magnitudes, chains them on a
//! RoadCurve, publishes the procedural mesh with build_mesh(), and joins
//! GLB props along the curve with add_props()/stamp_prop().

use gdscript::{GodotObject, Interpreter, Value, Vec3Val, CtorFn};
use crate::{join_along, stamp, JoinParams, PropInstance};
use roads::junction::{detect, weld_endpoints, Junction, JunctionKind, JunctionSettings};
use roads::model::{Alignment, BridgeStyle, LaneDir, RoadCurve, RoadPoint, TrafficUpdate};
use roads::terrain::TerrainState;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
/* ------------------------------------------------------------------ */
/* Scene state (owned by the app, shared with the interpreter)         */
/* ------------------------------------------------------------------ */

/// The GRG `RoadManager` node properties (defaults match road_manager.gd).
#[derive(Clone, Debug)]
pub struct ManagerState {
    /// meters between road loop cuts (GRG RoadSegment.DEFAULT_DENSITY = 4.0)
    pub density: f64,
    /// -1 = underside not generated
    pub underside_thickness: f64,
    /// group name for generated lanes
    pub ai_lane_group: String,
    /// auto refresh on transforms (GRG editor toggle)
    pub auto_refresh: bool,
}

impl Default for ManagerState {
    fn default() -> Self {
        Self {
            density: 4.0,
            underside_thickness: -1.0,
            ai_lane_group: "road_lanes".into(),
            auto_refresh: true,
        }
    }
}

#[derive(Default)]
pub struct SceneState {
    pub curves: Vec<RoadCurve>,
    pub props: Vec<PropInstance>,
    pub console: Vec<String>,
    pub dirty: bool,
    pub run_counter: u64,
    /// auto-detected junctions (recomputed on every scene change)
    pub junctions: Vec<Junction>,
    pub junction_settings: JunctionSettings,
    /// per-junction kind overrides keyed by (round(x), round(z)) meters
    pub junction_override: HashMap<(i64, i64), JunctionKind>,
    /// per-junction prefab overrides: "procedural" forces the n-gon patch,
    /// any other value forces that prefab id
    pub prefab_override: HashMap<(i64, i64), String>,
    /// the GRG RoadManager node (material/density/AI group defaults)
    pub manager: ManagerState,
    /// generated AI lanes (GRG RoadLane graph) — regenerated on refresh
    pub lanes: Vec<roads::lane::RoadLane>,
    /// "Make RoadLanes Editable" state (lanes shown + kept on rebuild)
    pub lanes_editable: bool,
    /// the RoadTerrain3DConnector (terrain hills + flatten-under-roads)
    pub terrain: TerrainState,
    /// R11 enrichment geometry (procedural buildings — world space),
    /// appended to the road mesh parts at scene build time
    pub enrich: Vec<roads::mesh3d::MeshPart>,
}

impl Clone for SceneState {
    fn clone(&self) -> Self {
        Self {
            curves: self.curves.clone(),
            props: self.props.clone(),
            console: self.console.clone(),
            dirty: self.dirty,
            run_counter: self.run_counter,
            junctions: self.junctions.clone(),
            junction_settings: self.junction_settings.clone(),
            junction_override: self.junction_override.clone(),
            prefab_override: self.prefab_override.clone(),
            manager: self.manager.clone(),
            lanes: self.lanes.clone(),
            lanes_editable: self.lanes_editable,
            terrain: self.terrain.clone(),
            enrich: Vec::new(),
        }
    }
}

impl SceneState {
    pub fn log(&mut self, msg: impl Into<String>) {
        if self.console.len() >= 400 {
            self.console.remove(0);
        }
        self.console.push(msg.into());
    }

    pub fn clear(&mut self) {
        self.curves.clear();
        self.props.clear();
        self.enrich.clear();
        self.junctions.clear();
        self.lanes.clear();
        self.dirty = true;
    }

    /// Regenerate the AI lane graph from the current curves (GRG
    /// `generate_lane_segments` + `update_lane_seg_connections`).
    pub fn recompute_lanes(&mut self) {
        self.lanes = roads::lane::generate_lanes(&self.curves, 4.0);
    }

    /// Re-run auto junction detection, apply manual overrides, then run the
    /// WFC prefab collapse (GLB tiles when the leg geometry fits).
    /// Also regenerates the AI lane graph so every scene change publishes
    /// fresh lanes (the single refresh entry point).
    pub fn recompute_junctions(&mut self) {
        self.junctions = detect(&self.curves, &self.junction_settings);
        self.recompute_lanes();
        for j in &mut self.junctions {
            let key = (j.x.round() as i64, j.z.round() as i64);
            if let Some(k) = self.junction_override.get(&key) {
                j.kind = *k;
                // rebuild the outline for the overridden kind
                let s = self.junction_settings.clone();
                let max_half = j.legs.iter().map(|l| l.half_width).fold(0.0f64, f64::max);
                match k {
                    JunctionKind::Crossing => {
                        let stop = (max_half * 1.4 + 1.0).max(3.0);
                        let mut outline = Vec::with_capacity(j.legs.len() * 2);
                        let mut legs = j.legs.clone();
                        legs.sort_by(|a, b| {
                            let aa = a.out_dir.1.atan2(a.out_dir.0);
                            let bb = b.out_dir.1.atan2(b.out_dir.0);
                            aa.partial_cmp(&bb).unwrap_or(std::cmp::Ordering::Equal)
                        });
                        for l in &legs {
                            let (px, pz) = (-l.out_dir.1, l.out_dir.0);
                            let (fx, fz) = (j.x + l.out_dir.0 * stop, j.z + l.out_dir.1 * stop);
                            outline.push((fx - px * l.half_width, fz - pz * l.half_width));
                            outline.push((fx + px * l.half_width, fz + pz * l.half_width));
                        }
                        j.outline = outline;
                        j.island = None;
                        j.ring_outer = 0.0;
                    }
                    JunctionKind::Roundabout => {
                        let ring_w = (max_half * 2.2).max(7.0);
                        j.ring_outer = s.island_radius + ring_w;
                        let mut outline = Vec::with_capacity(24);
                        for k in 0..24 {
                            let a = k as f64 / 24.0 * std::f64::consts::TAU;
                            outline.push((j.x + a.cos() * j.ring_outer, j.z + a.sin() * j.ring_outer));
                        }
                        j.outline = outline;
                        j.island = Some((j.x, j.z, s.island_radius));
                    }
                }
            }
        }
        // ---- WFC prefab collapse (after kind overrides) ----
        for j in &mut self.junctions {
            let key = (j.x.round() as i64, j.z.round() as i64);
            let forced = self.prefab_override.get(&key).cloned();
            let max_half = j.legs.iter().map(|l| l.half_width).fold(0.0f64, f64::max);
            if let Some(fid) = forced {
                if fid == "procedural" {
                    j.prefab = None;
                } else if let Some(def) = roads::prefab::by_id(&fid) {
                    // manual: best-effort orientation, width-scaled
                    let leg_dirs: Vec<(f64, f64)> = j.legs.iter().map(|l| l.out_dir).collect();
                    let mut place = roads::prefab::collapse(&leg_dirs, max_half, def.roundabout, true, self.junction_settings.island_radius)
                        .unwrap_or(roads::prefab::PrefabPlacement {
                            id: def.id.into(),
                            yaw: 0.0,
                            scale: (max_half / def.half_arm).clamp(0.12, 2.5),
                            flip: false,
                        });
                    place.id = def.id.into();
                    j.prefab = Some(place);
                    // footprint outline = rotated square of the prefab extent
                    let half = def.half_arm * j.prefab.as_ref().unwrap().scale;
                    let yw = j.prefab.as_ref().unwrap().yaw;
                    let (cc, ss) = (yw.cos(), yw.sin());
                    let mut outline = Vec::new();
                    for (lx, lz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                        outline.push((
                            j.x + lx * cc - lz * ss,
                            j.z + lx * ss + lz * cc,
                        ));
                    }
                    j.outline = outline;
                    if j.kind == JunctionKind::Roundabout {
                        j.island = None;
                    }
                }
            } else if self.junction_settings.use_prefabs {
                let leg_dirs: Vec<(f64, f64)> = j.legs.iter().map(|l| l.out_dir).collect();
                let roundabout = j.kind == JunctionKind::Roundabout;
                j.prefab = roads::prefab::collapse(
                    &leg_dirs,
                    max_half,
                    roundabout,
                    self.junction_settings.prefabs_manual,
                    self.junction_settings.island_radius,
                );
                if let Some(p) = &j.prefab {
                    if let Some(def) = roads::prefab::by_id(&p.id) {
                        let half = def.half_arm * p.scale;
                        let (cc, ss) = p.yaw.sin_cos();
                        let mut outline = Vec::new();
                        for (lx, lz) in [(-half, -half), (half, -half), (half, half), (-half, half)] {
                            outline.push((j.x + lx * cc - lz * ss, j.z + lx * ss + lz * cc));
                        }
                        j.outline = outline;
                        if j.kind == JunctionKind::Roundabout {
                            j.island = None;
                        }
                    }
                }
            } else {
                j.prefab = None;
            }
        }
    }

    /// Weld coincident curve endpoints (the auto-join) and recompute.
    pub fn auto_join(&mut self) -> usize {
        let welded = weld_endpoints(&mut self.curves, self.junction_settings.snap_m);
        self.recompute_junctions();
        welded
    }
}

pub type SharedScene = Rc<RefCell<SceneState>>;

/* ------------------------------------------------------------------ */
/* RoadPoint object                                                    */
/* ------------------------------------------------------------------ */

struct RoadPointObj {
    pt: RoadPoint,
}

fn num_of(v: &Value) -> Option<f64> {
    v.num()
}

fn get_f(o: &dyn GodotObject, name: &str, fallback: f64) -> f64 {
    o.get(name).and_then(|v| v.num()).unwrap_or(fallback)
}

/// Extract a RoadPoint from a script object by reading its properties
/// (keeps the bridge independent of concrete types across the trait).
fn point_from_obj(o: &dyn GodotObject) -> Result<RoadPoint, String> {
    if o.class_name() != "RoadPoint" {
        return Err(format!("expected a RoadPoint, got {}", o.class_name()));
    }
    let pos = o
        .get("position")
        .and_then(|v| v.as_vec3())
        .unwrap_or(Vec3Val::ZERO);
    Ok(RoadPoint {
        name: o.get("name").and_then(|v| v.as_str()).unwrap_or_default(),
        x: pos.x as f64,
        z: pos.z as f64,
        elev: pos.y as f64,
        bearing: get_f(o, "bearing", 0.0),
        prior_mag: get_f(o, "prior_mag", 30.0).max(0.5),
        next_mag: get_f(o, "next_mag", 30.0).max(0.5),
        lanes: o.get("lanes").and_then(|v| v.as_int()).unwrap_or(2).max(1) as usize,
        lane_width: get_f(o, "lane_width", 3.5).max(1.0),
        shoulder_l: get_f(o, "shoulder_l", 0.0),
        shoulder_r: get_f(o, "shoulder_r", 0.0),
        median: get_f(o, "median", 0.0).max(0.0),
        pathway_l: get_f(o, "pathway_l", 0.0).max(0.0),
        pathway_r: get_f(o, "pathway_r", 0.0).max(0.0),
        oneway: o.get("oneway").map(|v| v.truthy()).unwrap_or(false),
        visible: o.get("visible").map(|v| v.truthy()).unwrap_or(true),
        locked: o.get("locked").map(|v| v.truthy()).unwrap_or(false),
        dirt: o.get("dirt").map(|v| v.truthy()).unwrap_or(false),
        surface: o.get("surface").and_then(|v| v.as_int()).unwrap_or(0) as u8,
        auto_bearing: o.get("auto_bearing").map(|v| v.truthy()).unwrap_or(false),
        rev_lanes: o.get("rev_lanes").and_then(|v| v.as_int()).map(|v| v.max(0) as usize).unwrap_or(0),
        auto_lanes: o.get("auto_lanes").map(|v| v.truthy()).unwrap_or(true),
        terminated: o.get("terminated").map(|v| v.truthy()).unwrap_or(false),
        create_geo: o.get("create_geo").map(|v| v.truthy()).unwrap_or(true),
        flatten_terrain: o.get("flatten_terrain").map(|v| v.truthy()).unwrap_or(true),
        gutter_profile: (
            get_f(o, "gutter_profile_x", 2.0),
            get_f(o, "gutter_profile_y", -0.5),
        ),
        alignment: if o.get("alignment").and_then(|v| v.as_int()).unwrap_or(0) == 1 {
            Alignment::Divider
        } else {
            Alignment::Geometric
        },
        underside_thickness: get_f(o, "underside_thickness", -1.0),
        ..Default::default()
    })
}

impl GodotObject for RoadPointObj {
    fn class_name(&self) -> &'static str {
        "RoadPoint"
    }

    fn get(&self, name: &str) -> Option<Value> {
        let p = &self.pt;
        Some(match name {
            "position" => Value::Vec3(Vec3Val::new(p.x as f32, p.elev as f32, p.z as f32)),
            "bearing" => Value::Float(p.bearing),
            "prior_mag" => Value::Float(p.prior_mag),
            "next_mag" => Value::Float(p.next_mag),
            "lanes" => Value::Int(p.lanes as i64),
            "lane_width" => Value::Float(p.lane_width),
            "shoulder_l" => Value::Float(p.shoulder_l),
            "shoulder_r" => Value::Float(p.shoulder_r),
            "median" => Value::Float(p.median),
            "pathway_l" => Value::Float(p.pathway_l),
            "pathway_r" => Value::Float(p.pathway_r),
            "oneway" => Value::Bool(p.oneway),
            "visible" => Value::Bool(p.visible),
            "locked" => Value::Bool(p.locked),
            "dirt" => Value::Bool(p.dirt),
            "auto_bearing" => Value::Bool(p.auto_bearing),
            // ---- GRG RoadPoint property set ----
            "rev_lanes" => Value::Int(p.rev_lanes as i64),
            "auto_lanes" => Value::Bool(p.auto_lanes),
            "terminated" => Value::Bool(p.terminated),
            "create_geo" => Value::Bool(p.create_geo),
            "flatten_terrain" => Value::Bool(p.flatten_terrain),
            "gutter_profile_x" => Value::Float(p.gutter_profile.0),
            "gutter_profile_y" => Value::Float(p.gutter_profile.1),
            "alignment" => Value::Int(match p.alignment {
                Alignment::Geometric => 0,
                Alignment::Divider => 1,
            }),
            "underside_thickness" => Value::Float(p.underside_thickness),
            "traffic_dir" => Value::str(
                p.traffic_dir()
                    .iter()
                    .map(|d| d.as_char().to_string())
                    .collect::<Vec<_>>()
                    .join(","),
            ),
            "name" => Value::str(p.name.clone()),
            _ => return None,
        })
    }

    fn set(&mut self, name: &str, v: Value) -> Result<(), String> {
        let p = &mut self.pt;
        match name {
            "position" => {
                let q = v
                    .as_vec3()
                    .ok_or_else(|| "position expects a Vector3".to_string())?;
                p.x = q.x as f64;
                p.elev = q.y as f64;
                p.z = q.z as f64;
            }
            "bearing" => p.bearing = num_of(&v).ok_or("bearing expects a number")?,
            "prior_mag" => p.prior_mag = num_of(&v).ok_or("prior_mag expects a number")?.max(0.5),
            "next_mag" => p.next_mag = num_of(&v).ok_or("next_mag expects a number")?.max(0.5),
            "lanes" => p.lanes = v.as_int().ok_or("lanes expects an int")?.max(1) as usize,
            "lane_width" => p.lane_width = num_of(&v).ok_or("lane_width expects a number")?.max(1.0),
            "shoulder_l" => p.shoulder_l = num_of(&v).unwrap_or(0.0).max(0.0),
            "shoulder_r" => p.shoulder_r = num_of(&v).unwrap_or(0.0).max(0.0),
            "median" => p.median = num_of(&v).unwrap_or(0.0).clamp(0.0, 12.0),
            "pathway_l" => p.pathway_l = num_of(&v).unwrap_or(0.0).clamp(0.0, 8.0),
            "pathway_r" => p.pathway_r = num_of(&v).unwrap_or(0.0).clamp(0.0, 8.0),
            "oneway" => p.oneway = v.truthy(),
            "visible" => p.visible = v.truthy(),
            "locked" => p.locked = v.truthy(),
            "dirt" => p.dirt = v.truthy(),
            "auto_bearing" => p.auto_bearing = v.truthy(),
            // ---- GRG RoadPoint property set ----
            "rev_lanes" => {
                let n = v.as_int().ok_or("rev_lanes expects an int")?.max(0) as usize;
                p.rev_lanes = n.min(p.lanes.saturating_sub(1));
                p.oneway = p.rev_lanes == 0;
            }
            "auto_lanes" => p.auto_lanes = v.truthy(),
            "terminated" => p.terminated = v.truthy(),
            "create_geo" => p.create_geo = v.truthy(),
            "flatten_terrain" => p.flatten_terrain = v.truthy(),
            "gutter_profile_x" => p.gutter_profile.0 = num_of(&v).unwrap_or(2.0),
            "gutter_profile_y" => p.gutter_profile.1 = num_of(&v).unwrap_or(-0.5),
            "alignment" => {
                p.alignment = if v.as_int().unwrap_or(0) == 1 {
                    Alignment::Divider
                } else {
                    Alignment::Geometric
                };
            }
            "underside_thickness" => p.underside_thickness = num_of(&v).unwrap_or(-1.0),
            "traffic_dir" => {
                // "R,R,F,F" style — REVERSE entries must come first (GRG)
                let s = v.as_str().ok_or("traffic_dir expects a string like 'R,R,F,F'")?;
                let dirs: Vec<char> = s
                    .chars()
                    .filter(|c| LaneDir::from_char(*c).is_some())
                    .collect();
                if dirs.is_empty() {
                    return Err("traffic_dir needs at least one lane".into());
                }
                p.lanes = dirs.len();
                p.rev_lanes = dirs.iter().filter(|c| **c == 'R').count();
                p.oneway = p.rev_lanes == 0;
            }
            "name" => p.name = v.as_str().unwrap_or_default(),
            other => return Err(format!("RoadPoint has no property '{other}'")),
        }
        Ok(())
    }

    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, String> {
        match name {
            "to_string" => Ok(Value::str(self.describe())),
            // GRG update_traffic_dir(TrafficUpdate) — the lane +/- buttons
            "update_traffic_dir" => {
                let act = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("update_traffic_dir('add_forward'|'add_reverse'|'rem_forward'|'rem_reverse')")?
                    .to_string();
                let op = match act.as_str() {
                    "add_forward" | "ADD_FORWARD" => TrafficUpdate::AddForward,
                    "add_reverse" | "ADD_REVERSE" => TrafficUpdate::AddReverse,
                    "rem_forward" | "REM_FORWARD" => TrafficUpdate::RemForward,
                    "rem_reverse" | "REM_REVERSE" => TrafficUpdate::RemReverse,
                    other => return Err(format!("unknown traffic update '{other}'")),
                };
                let ok = self.pt.update_traffic_dir(op);
                Ok(Value::Bool(ok))
            }
            other => Err(format!(
                "RoadPoint has no method '{other}' — set properties, then curve.add_point(p)"
            )),
        }
    }

    fn describe(&self) -> String {
        let p = &self.pt;
        format!(
            "<RoadPoint pos=({:.1}, {:.1}, {:.1}) lanes={}>",
            p.x, p.elev, p.z, p.lanes
        )
    }
}

/* ------------------------------------------------------------------ */
/* RoadCurve object                                                    */
/* ------------------------------------------------------------------ */

struct RoadCurveObj {
    scene: SharedScene,
    curve: RoadCurve,
}

impl GodotObject for RoadCurveObj {
    fn class_name(&self) -> &'static str {
        "RoadCurve"
    }

    fn get(&self, name: &str) -> Option<Value> {
        Some(match name {
            "name" => Value::str(self.curve.name.clone()),
            "closed" => Value::Bool(self.curve.closed),
            "length" => Value::Float(self.curve.length_m()),
            "point_count" => Value::Int(self.curve.points.len() as i64),
            // ---- GRG RoadContainer property set ----
            "visible" => Value::Bool(self.curve.visible),
            "create_geo" => Value::Bool(self.curve.create_geo),
            "density" => Value::Float(self.curve.density),
            "use_lowpoly_preview" => Value::Bool(self.curve.use_lowpoly_preview),
            "flatten_terrain" => Value::Bool(self.curve.flatten_terrain),
            "underside_thickness" => Value::Float(self.curve.underside_thickness),
            "generate_ai_lanes" => Value::Bool(self.curve.generate_ai_lanes),
            "ai_lane_group" => Value::str(self.curve.ai_lane_group.clone()),
            "auto_free_vehicles" => Value::Bool(self.curve.auto_free_vehicles),
            "draw_lanes_editor" => Value::Bool(self.curve.draw_lanes_editor),
            "draw_lanes_game" => Value::Bool(self.curve.draw_lanes_game),
            "create_edge_curves" => Value::Bool(self.curve.create_edge_curves),
            // ---- R12 RoadArchitect systems ----
            "guardrail" => Value::Bool(self.curve.guardrail),
            "tunnel" => Value::Bool(self.curve.tunnel),
            "cable_barrier" => Value::Bool(self.curve.cable_barrier),
            "bridge_style" => Value::str(match self.curve.bridge_style {
                BridgeStyle::Girder => "girder",
                BridgeStyle::BoxGirder => "box_girder",
                BridgeStyle::Arch => "arch",
                BridgeStyle::Suspension => "suspension",
            }),
            "debug" => Value::Bool(self.curve.debug),
            _ => return None,
        })
    }

    fn set(&mut self, name: &str, v: Value) -> Result<(), String> {
        match name {
            "name" => self.curve.name = v.as_str().unwrap_or_default(),
            "closed" => self.curve.closed = v.truthy(),
            // ---- GRG RoadContainer property set ----
            "visible" => self.curve.visible = v.truthy(),
            "create_geo" => self.curve.create_geo = v.truthy(),
            "density" => self.curve.density = v.num().unwrap_or(-1.0),
            "use_lowpoly_preview" => self.curve.use_lowpoly_preview = v.truthy(),
            "flatten_terrain" => self.curve.flatten_terrain = v.truthy(),
            "underside_thickness" => self.curve.underside_thickness = v.num().unwrap_or(-1.0),
            "generate_ai_lanes" => self.curve.generate_ai_lanes = v.truthy(),
            "ai_lane_group" => self.curve.ai_lane_group = v.as_str().unwrap_or_else(|| "road_lanes".into()),
            "auto_free_vehicles" => self.curve.auto_free_vehicles = v.truthy(),
            "draw_lanes_editor" => self.curve.draw_lanes_editor = v.truthy(),
            "draw_lanes_game" => self.curve.draw_lanes_game = v.truthy(),
            "create_edge_curves" => self.curve.create_edge_curves = v.truthy(),
            // ---- R12 RoadArchitect systems ----
            "guardrail" => self.curve.guardrail = v.truthy(),
            "tunnel" => self.curve.tunnel = v.truthy(),
            "cable_barrier" => self.curve.cable_barrier = v.truthy(),
            "bridge_style" => {
                let Some(s) = v.as_str() else {
                    return Err(
                        "bridge_style expects a string: girder | box | boxgirder | arch | suspension"
                            .into(),
                    );
                };
                self.curve.bridge_style = match s.to_lowercase().as_str() {
                    "girder" => BridgeStyle::Girder,
                    "box" | "boxgirder" => BridgeStyle::BoxGirder,
                    "arch" => BridgeStyle::Arch,
                    "suspension" => BridgeStyle::Suspension,
                    other => {
                        return Err(format!(
                            "bridge_style '{other}' unknown — use girder | box | boxgirder | arch | suspension"
                        ))
                    }
                };
            }
            "debug" => self.curve.debug = v.truthy(),
            other => return Err(format!("RoadCurve has no property '{other}'")),
        }
        Ok(())
    }

    fn call(&mut self, name: &str, args: &[Value]) -> Result<Value, String> {
        match name {
            "add_point" => {
                let Some(first) = args.first() else {
                    return Err("add_point expects a RoadPoint".into());
                };
                let v = first.clone();
                let Value::Obj(o) = v else {
                    return Err("add_point expects a RoadPoint".into());
                };
                let pt = point_from_obj(&*o.borrow())?;
                self.curve.add_point(pt);
                self.curve.auto_orient();
                Ok(Value::Nil)
            }
            "build_mesh" | "build" => {
                if self.curve.points.len() < 2 {
                    return Err(format!(
                        "curve '{}' needs at least 2 points before build_mesh()",
                        self.curve.name
                    ));
                }
                self.curve.auto_orient();
                let len = self.curve.length_m();
                let pts = self.curve.points.len();
                let mut scene = self.scene.borrow_mut();
                if let Some(existing) = scene.curves.iter_mut().find(|c| c.name == self.curve.name) {
                    *existing = self.curve.clone();
                } else {
                    scene.curves.push(self.curve.clone());
                }
                let before = scene.junctions.len();
                scene.recompute_junctions();
                let new_junctions: Vec<(String, f64, f64, usize)> = scene.junctions[before..]
                    .iter()
                    .map(|j| (j.kind.label().to_string(), j.x, j.z, j.legs.len()))
                    .collect();
                for (kind, x, z, legs) in new_junctions {
                    scene.log(format!(
                        "[auto-join] {kind} @ ({x:.0}, {z:.0}) — {legs} road legs"
                    ));
                }
                scene.dirty = true;
                let name = self.curve.name.clone();
                scene.log(format!(
                    "[road] built '{name}': {pts} points, {len:.0} m, lanes={}",
                    self.curve.points.first().map(|p| p.lanes).unwrap_or(2)
                ));
                Ok(Value::Nil)
            }
            "add_props" => {
                // add_props(asset, spacing, align)
                let asset = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("add_props(asset: String, spacing: float, align: bool)")?
                    .to_string();
                let spacing = args.get(1).and_then(Value::num).unwrap_or(30.0).max(1.0);
                let align = args.get(2).map(|v| v.truthy()).unwrap_or(true);
                let lateral = self.curve.paved_width() / 2.0 + 2.5;
                self.join_internal(&asset, spacing, lateral, true, align, false)
            }
            "add_props_offset" => {
                // add_props_offset(asset, spacing, lateral, alternate, align)
                let asset = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("add_props_offset(asset, spacing, lateral, alternate, align)")?
                    .to_string();
                let spacing = args.get(1).and_then(Value::num).unwrap_or(30.0).max(1.0);
                let lateral = args.get(2).and_then(Value::num).unwrap_or(6.0);
                let alternate = args.get(3).map(|v| v.truthy()).unwrap_or(true);
                let align = args.get(4).map(|v| v.truthy()).unwrap_or(true);
                self.join_internal(&asset, spacing, lateral, alternate, align, false)
            }
            "stamp_prop" => {
                // stamp_prop(asset, t, lateral)
                let asset = args
                    .first()
                    .and_then(Value::as_str)
                    .ok_or("stamp_prop(asset: String, t: float, lateral: float)")?
                    .to_string();
                let t = args.get(1).and_then(Value::num).unwrap_or(0.0);
                let lateral = args.get(2).and_then(Value::num).unwrap_or(0.0);
                if self.curve.points.len() < 2 {
                    return Err(format!(
                        "curve '{}' needs at least 2 points before stamping props",
                        self.curve.name
                    ));
                }
                let Some(prop) = stamp(&self.curve, &asset, t, lateral, true, 1.0) else {
                    return Err(format!("stamp_prop failed on curve '{}'", self.curve.name));
                };
                let pos = prop.pos;
                let mut scene = self.scene.borrow_mut();
                scene.props.push(prop);
                scene.dirty = true;
                scene.log(format!(
                    "[props] stamped {asset} at t={t:.2} on '{}' → ({:.0}, {:.0})",
                    self.curve.name, pos.x, pos.z
                ));
                Ok(Value::Nil)
            }
            "clear_props" => {
                let owner = self.curve.name.clone();
                let mut scene = self.scene.borrow_mut();
                let before = scene.props.len();
                scene.props.retain(|p| p.owner != owner);
                let removed = before - scene.props.len();
                scene.dirty = true;
                scene.log(format!("[props] cleared {removed} props from '{owner}'"));
                Ok(Value::Nil)
            }
            "length" => Ok(Value::Float(self.curve.length_m())),
            "point_count" => Ok(Value::Int(self.curve.points.len() as i64)),
            // GRG "Bake RoadLanes" / "Make RoadLanes Editable"
            "make_lanes_editable" => {
                let mut scene = self.scene.borrow_mut();
                scene.lanes_editable = true;
                let n = scene.lanes.iter().filter(|l| l.container == self.lane_index()).count();
                scene.log(format!(
                    "[lanes] made {n} RoadLane(s) of '{}' visible + editable (GRG Make RoadLanes Editable)",
                    self.curve.name
                ));
                Ok(Value::Int(n as i64))
            }
            "lanes_info" => {
                let scene = self.scene.borrow();
                let mine: Vec<&roads::lane::RoadLane> =
                    scene.lanes.iter().filter(|l| l.container == self.lane_index()).collect();
                Ok(Value::str(format!(
                    "<RoadLanes '{}' count={} connected={}>",
                    self.curve.name,
                    mine.len(),
                    mine.iter().filter(|l| l.lane_next.is_some()).count(),
                )))
            }
            "to_string" => Ok(Value::str(self.describe())),
            other => Err(format!("RoadCurve has no method '{other}'")),
        }
    }

    fn describe(&self) -> String {
        format!(
            "<RoadCurve '{}' points={} length={:.0}m>",
            self.curve.name,
            self.curve.points.len(),
            self.curve.length_m()
        )
    }
}

impl RoadCurveObj {
    fn lane_index(&self) -> usize {
        // resolve our index in the scene (kept fresh for lane helpers)
        self.scene
            .borrow()
            .curves
            .iter()
            .position(|c| c.name == self.curve.name)
            .unwrap_or(usize::MAX)
    }

    fn join_internal(
        &mut self,
        asset: &str,
        spacing: f64,
        lateral: f64,
        alternate: bool,
        align: bool,
        random_yaw: bool,
    ) -> Result<Value, String> {
        if self.curve.points.len() < 2 {
            return Err(format!(
                "curve '{}' needs at least 2 points before add_props()",
                self.curve.name
            ));
        }
        let seed = {
            let mut scene = self.scene.borrow_mut();
            scene.run_counter += 1;
            scene.run_counter.wrapping_mul(2654435761).wrapping_add(1337)
        };
        let params = JoinParams {
            spacing,
            lateral,
            alternate,
            align,
            scale: 1.0,
            jitter: 0.12,
            seed,
            random_yaw,
        };
        let props = join_along(&self.curve, asset, &params);
        let n = props.len();
        let mut scene = self.scene.borrow_mut();
        scene.props.extend(props);
        scene.dirty = true;
        scene.log(format!(
            "[props] joined {n} '{asset}' along '{}' (spacing {spacing:.0} m, lateral {lateral:.1} m)",
            self.curve.name
        ));
        Ok(Value::Int(n as i64))
    }
}

/* ------------------------------------------------------------------ */
/* Constructor registration                                            */
/* ------------------------------------------------------------------ */

pub fn register(interp: &mut Interpreter, scene: &SharedScene) {
    interp.register_fn(
        "RoadPoint",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            Ok(Value::Obj(std::rc::Rc::new(RefCell::new(RoadPointObj {
                pt: RoadPoint::default(),
            }))))
        }) as CtorFn,
    );

    let s2 = scene.clone();
    interp.register_fn(
        "RoadCurve",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let given = args.first().and_then(Value::as_str).unwrap_or_default();
            let name = if given.is_empty() {
                let mut sc = s2.borrow_mut();
                sc.run_counter += 1;
                format!("Path {}", sc.run_counter)
            } else {
                given.to_string()
            };
            Ok(Value::Obj(std::rc::Rc::new(RefCell::new(RoadCurveObj {
                scene: s2.clone(),
                curve: RoadCurve::new(name),
            }))))
        }) as CtorFn,
    );

    let s3 = scene.clone();
    interp.register_fn(
        "clear_scene",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let mut sc = s3.borrow_mut();
            sc.clear();
            sc.log("[scene] cleared");
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s4 = scene.clone();
    interp.register_fn(
        "scene_info",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let sc = s4.borrow();
            Ok(Value::str(format!(
                "<scene curves={} props={}>",
                sc.curves.len(),
                sc.props.len()
            )))
        }) as CtorFn,
    );

    // ---- junction auto-detection API (RoadKit extension) ----

    let s5 = scene.clone();
    interp.register_fn(
        "auto_junction",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let on = args.first().map(|v| v.truthy()).unwrap_or(true);
            let mut sc = s5.borrow_mut();
            sc.junction_settings.enabled = on;
            sc.recompute_junctions();
            sc.log(format!("[junctions] auto detection {}", if on { "ON" } else { "OFF" }));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s6 = scene.clone();
    interp.register_fn(
        "auto_roundabout",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let on = args.first().map(|v| v.truthy()).unwrap_or(true);
            let mut sc = s6.borrow_mut();
            sc.junction_settings.auto_roundabout = on;
            sc.recompute_junctions();
            sc.log(format!(
                "[junctions] 3+ leg junctions become {}",
                if on { "roundabouts" } else { "crossings" }
            ));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s7 = scene.clone();
    interp.register_fn(
        "set_snap_m",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let m = args.first().and_then(Value::num).unwrap_or(8.0).clamp(0.5, 60.0);
            let mut sc = s7.borrow_mut();
            sc.junction_settings.snap_m = m;
            sc.recompute_junctions();
            sc.log(format!("[junctions] snap threshold {m:.1} m (GRG connection tool default 25 m editor / 8 m visual)"));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s9 = scene.clone();
    interp.register_fn(
        "set_island_radius",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let r = args.first().and_then(Value::num).unwrap_or(5.0).clamp(2.0, 30.0);
            let mut sc = s9.borrow_mut();
            sc.junction_settings.island_radius = r;
            sc.recompute_junctions();
            sc.log(format!("[junctions] roundabout island radius {r:.1} m"));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s8 = scene.clone();
    interp.register_fn(
        "junction_info",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let sc = s8.borrow();
            if sc.junctions.is_empty() {
                return Ok(Value::str("no junctions detected"));
            }
            let mut lines = Vec::new();
            for (i, j) in sc.junctions.iter().enumerate() {
                let mut names: Vec<String> = Vec::new();
                for l in &j.legs {
                    let n = sc
                        .curves
                        .get(l.curve)
                        .map(|c| c.name.clone())
                        .unwrap_or_else(|| format!("curve {}", l.curve));
                    if !names.contains(&n) {
                        names.push(n);
                    }
                }
                let tile = j
                    .prefab
                    .as_ref()
                    .map(|p| format!(" · WFC tile {}", p.id))
                    .unwrap_or_else(|| " · procedural patch".to_string());
                lines.push(format!(
                    "  #{} {} @ ({:.0}, {:.0}) — {} legs from [{}]{}",
                    i + 1,
                    j.kind.label(),
                    j.x,
                    j.z,
                    j.legs.len(),
                    names.join(", "),
                    tile
                ));
            }
            Ok(Value::str(format!(
                "{} junction(s):\n{}",
                sc.junctions.len(),
                lines.join("\n")
            )))
        }) as CtorFn,
    );

    let s9 = scene.clone();
    interp.register_fn(
        "auto_join",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let mut sc = s9.borrow_mut();
            let welded = sc.auto_join();
            sc.log(format!(
                "[auto-join] welded {welded} endpoint group(s) — like Godot's connection tool snap"
            ));
            Ok(Value::Int(welded as i64))
        }) as CtorFn,
    );

    // ---- WFC junction prefabs (GRG custom-container GLB tiles) ----

    let s10 = scene.clone();
    interp.register_fn(
        "junction_prefabs",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let on = args.first().map(|v| v.truthy()).unwrap_or(true);
            let mut sc = s10.borrow_mut();
            sc.junction_settings.use_prefabs = on;
            sc.recompute_junctions();
            let n = sc.junctions.iter().filter(|j| j.prefab.is_some()).count();
            sc.log(format!(
                "[prefabs] WFC tile matching {} — {n} junction(s) collapsed to GLB prefabs",
                if on { "ON" } else { "OFF (procedural patches)" }
            ));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s11 = scene.clone();
    interp.register_fn(
        "junction_prefab_ramps",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let on = args.first().map(|v| v.truthy()).unwrap_or(true);
            let mut sc = s11.borrow_mut();
            sc.junction_settings.prefabs_manual = on;
            sc.recompute_junctions();
            sc.log(format!(
                "[prefabs] splitter/on-ramp/off-ramp tiles {} in the WFC candidate set",
                if on { "INCLUDED" } else { "EXCLUDED" }
            ));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    // ---- RoadTerrain3DConnector (GRG connectors/terrain3d_road_connector.gd) ----

    let s12 = scene.clone();
    interp.register_fn(
        "set_terrain",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let on = args.first().map(|v| v.truthy()).unwrap_or(true);
            let mut sc = s12.borrow_mut();
            sc.terrain.enabled = on;
            let (amp, margin, falloff) = (sc.terrain.amplitude, sc.terrain.edge_margin, sc.terrain.edge_falloff);
            sc.log(format!(
                "[terrain] RoadTerrain3DConnector {} (hills {amp} m, flatten margin {margin:.1} m, falloff {falloff:.1} m)",
                if on { "added" } else { "removed" },
            ));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s13 = scene.clone();
    interp.register_fn(
        "set_terrain_hills",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let amp = args.first().and_then(Value::num).unwrap_or(6.0).clamp(0.0, 60.0);
            let wave = args.get(1).and_then(Value::num).unwrap_or(130.0).clamp(20.0, 2000.0);
            let mut sc = s13.borrow_mut();
            sc.terrain.amplitude = amp;
            sc.terrain.wavelength = wave;
            sc.log(format!("[terrain] hills amplitude {amp:.1} m, wavelength {wave:.0} m"));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    let s14 = scene.clone();
    interp.register_fn(
        "set_terrain_flatten",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let margin = args.first().and_then(Value::num).unwrap_or(0.5).clamp(0.0, 20.0);
            let falloff = args.get(1).and_then(Value::num).unwrap_or(2.0).clamp(0.5, 50.0);
            let mut sc = s14.borrow_mut();
            sc.terrain.edge_margin = margin;
            sc.terrain.edge_falloff = falloff;
            sc.log(format!("[terrain] flatten margin {margin:.1} m, falloff {falloff:.1} m (GRG APPROXIMATE method)"));
            Ok(Value::Nil)
        }) as CtorFn,
    );

    // ---- RoadLane / AI diagnostics ----

    let s15 = scene.clone();
    interp.register_fn(
        "lanes_info",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let sc = s15.borrow();
            if sc.lanes.is_empty() {
                return Ok(Value::str("no AI lanes — set generate_ai_lanes on a container + rebuild".to_string()));
            }
            let connected = sc.lanes.iter().filter(|l| l.lane_next.is_some()).count();
            let transitions = sc.lanes.iter().filter(|l| l.transition).count();
            Ok(Value::str(format!(
                "<RoadLanes total={} connected={} transitions={} group='{}'>",
                sc.lanes.len(),
                connected,
                transitions,
                sc.manager.ai_lane_group,
            )))
        }) as CtorFn,
    );

    let s16 = scene.clone();
    interp.register_fn(
        "make_lanes_editable",
        Box::new(move |_it: &mut Interpreter, _args: &[Value]| {
            let mut sc = s16.borrow_mut();
            sc.lanes_editable = true;
            let n = sc.lanes.len();
            sc.log(format!("[lanes] made all {n} RoadLane(s) visible + editable"));
            Ok(Value::Int(n as i64))
        }) as CtorFn,
    );

    let s17 = scene.clone();
    interp.register_fn(
        "set_lane_visibility",
        Box::new(move |_it: &mut Interpreter, args: &[Value]| {
            let editor = args.first().map(|v| v.truthy()).unwrap_or(true);
            let game = args.get(1).map(|v| v.truthy()).unwrap_or(false);
            let mut sc = s17.borrow_mut();
            for c in sc.curves.iter_mut() {
                c.draw_lanes_editor = editor;
                c.draw_lanes_game = game;
            }
            sc.log(format!("[lanes] draw_lanes_editor={editor} draw_lanes_game={game} on all containers"));
            Ok(Value::Nil)
        }) as CtorFn,
    );
}
