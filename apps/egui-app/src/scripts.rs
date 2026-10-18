//! Bundled .gd scripts (embedded into the binary AND shipped in assets/).

pub const DEMO: &str = include_str!("../../../assets/scripts/demo_highway.gd");
pub const RING: &str = include_str!("../../../assets/scripts/ring_loop.gd");
pub const GRID: &str = include_str!("../../../assets/scripts/grid_network.gd");
pub const JUNCTION: &str = include_str!("../../../assets/scripts/junction_test.gd");
pub const TEXTURES: &str = include_str!("../../../assets/scripts/texture_showcase.gd");
pub const TRAFFIC: &str = include_str!("../../../assets/scripts/traffic_junction.gd");
pub const BRT: &str = include_str!("../../../assets/scripts/brt_corridor.gd");
pub const SC_CITY: &str = include_str!("../../../assets/scenarios/city_grid.gd");
pub const SC_PLAZA: &str = include_str!("../../../assets/scenarios/roundabout_square.gd");
pub const SC_HWY: &str = include_str!("../../../assets/scenarios/highway_ramps.gd");
pub const SC_ZOOM: &str = include_str!("../../../assets/scenarios/texture_zoom.gd");
pub const SC_TAPER: &str = include_str!("../../../assets/scenarios/lane_taper.gd");
pub const SC_PREFAB: &str = include_str!("../../../assets/scenarios/prefab_4way.gd");
pub const SC_DAR: &str = include_str!("../../../assets/scenarios/dar_crossing.gd");

/// (label, filename, source)
pub fn all() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        ("Demo Highway", "demo_highway.gd", DEMO),
        ("Ring Loop", "ring_loop.gd", RING),
        ("Grid Network", "grid_network.gd", GRID),
        ("Junction Test", "junction_test.gd", JUNCTION),
        ("Texture Showcase", "texture_showcase.gd", TEXTURES),
        ("Traffic Junction", "traffic_junction.gd", TRAFFIC),
        ("BRT Mwendokasi", "brt_corridor.gd", BRT),
        ("City Grid", "city_grid.gd", SC_CITY),
        ("Roundabout Square", "roundabout_square.gd", SC_PLAZA),
        ("Highway Ramps", "highway_ramps.gd", SC_HWY),
        ("Texture Zoom", "texture_zoom.gd", SC_ZOOM),
        ("Lane Taper (4→2)", "lane_taper.gd", SC_TAPER),
        ("Prefab 4-way (WFC)", "prefab_4way.gd", SC_PREFAB),
        ("Dar es Salaam Crossing", "dar_crossing.gd", SC_DAR),
    ]
}

pub fn default_source() -> String {
    DEMO.to_string()
}

/// The .gd scripting API reference (shown in Help ▸ .gd scripting API).
pub fn api_text() -> &'static str {
    r#"RoadKit Godot — .gd scripting API
=================================
A GDScript subset runs inside the editor (parser + interpreter in the
`gdscript` crate). Scripts look and feel like Godot: `extends Node3D`,
`func _ready():`, `Vector3(...)`, `Klass.new()`, indentation blocks.

CLASSES
-------
RoadPoint — one GRG road point (like godot-road-generator's road_point.gd)
  .position : Vector3   (x east, y elevation, z south)   [get/set]
  .bearing  : float     travel direction, radians        [get/set]
  .prior_mag / .next_mag : float  Bezier handle lengths [get/set]
  .lanes    : int       TOTAL lane count                 [get/set]
  .rev_lanes : int      REVERSE (oncoming) lane count; traffic_dir is
                        stored REVERSE-first, FORWARD-last
  .auto_lanes : bool    auto-assign lane types from traffic_dir
  .traffic_dir : String "R,R,F,F" — settable shorthand for the lane
                        split (reads back the REVERSE-first array)
  .update_traffic_dir(op)  one of "add_forward" | "add_reverse" |
                        "rem_forward" | "rem_reverse" — the exact GRG
                        lane ops (refuse when lanes <= 1 or side empty)
  .terminated : bool    non-edge end (excluded from joins)
  .create_geo : bool    generate procedural geometry from this point
  .flatten_terrain : bool  flatten terrain under adjacent segments
  .gutter_profile_x / .gutter_profile_y : float  shoulder gutter (2.0, -0.5)
  .alignment : int      0 = Geometric (road on origin), 1 = Divider
                        (lane divider on origin)
  .underside_thickness : float  -1 = container / manager value
  .lane_width, .shoulder_l, .shoulder_r : float          [get/set]
  .oneway   : bool      derived: on when rev_lanes == 0   [get/set]
  .median   : float   center median width, m (>=3.0 = BRT cement + tactile)
  .pathway_l / .pathway_r : float  sidewalk widths, m    [get/set]
  .dirt     : bool    mud lanes + rough gravel shoulders  [get/set]
  .auto_bearing : bool  bearing auto-computed from neighbours (straight
                          roads with zero trig; GRG interpolates every
                          cross-section property between two RoadPoints,
                          so per-point lane edits taper smoothly: 4 lanes
                          here, 2 there — the extra lane wedges out)
  .visible  : bool    layer-engine eye                    [get/set]
  .locked   : bool    lock from plan-view dragging        [get/set]
  .name     : String                                      [get/set]

RoadCurve — a chain of RoadPoints (GRG's Road container)
  .name         : String                                 [get/set]
  .closed       : bool    close the loop                 [get/set]
  .new([name])  -> RoadCurve
  .add_point(p)           append a RoadPoint
  .build_mesh()           publish the procedural mesh to the scene
  .length()     -> float  arc length in meters
  .point_count()-> int
  .add_props(asset, spacing, align)
        join a GLB asset along the whole curve, Godot-style
        lateral defaults to pavement edge + 2.5 m, sides alternate
  .add_props_offset(asset, spacing, lateral, alternate, align)
  .stamp_prop(asset, t, lateral)
        place one prop at normalized distance t (0..1)
  .clear_props()          remove props owned by this curve

  RoadContainer properties (GRG inspector groups):
  .create_geo : bool         generate geometry for this container
  .density : float           loop-cut spacing, -1 = RoadManager value
  .use_lowpoly_preview : bool  fewer loop cuts while dragging
  .flatten_terrain : bool    flatten terrain under these roads
  .underside_thickness : float  -1 = manager value
  .generate_ai_lanes : bool  auto-generate the RoadLane graph
  .ai_lane_group : String    group name for generated lanes ("road_lanes")
  .auto_free_vehicles : bool free agents when a lane exits the tree
  .draw_lanes_editor : bool  visualize lanes in the editor
  .draw_lanes_game : bool    visualize lanes at runtime
  .create_edge_curves : bool left/center/right edge Path3Ds
  .debug : bool              verbose container diagnostics
  .make_lanes_editable()     promote the generated lanes to editable ones
  .lanes_info()    -> String  "<n> lanes in group '<group>'"

GLOBALS
-------
  print(...)              console output
  Vector3(x, y, z)        build a Vector3
  str(v), len(a)          convert / length
  range(a[, b])           int range for `for x in range(...)`
  randf(), randf_range(a, b)
  abs, floor, sqrt, min, max, clamp
  sin, cos, deg_to_rad, rad_to_deg
  clear_scene()           wipe curves + props
  scene_info()            quick scene summary
  set_terrain(on)         enable/disable the RoadTerrain3DConnector
  set_terrain_hills(amp, wave)
                          hill amplitude (m) + wavelength (m)
  set_terrain_flatten(margin, falloff)
                          flatten-under-roads edge margin + falloff
  lanes_info()            lane counts per container ("N lanes in group …")
  make_lanes_editable()   promote the generated lanes everywhere
  set_lane_visibility(editor, game)
                          draw_lanes_editor / draw_lanes_game for all

JUNCTIONS (auto-detected, RoadKit extension)
--------------------------------------------
  auto_junction(bool)     enable/disable detection (default ON)
  auto_roundabout(bool)   3+ leg junctions become roundabouts (default ON)
  set_snap_m(float)       endpoint snap threshold in meters (default 8)
  auto_join()             weld coincident endpoints now (returns count)
  junction_prefabs(true|false)
  WFC-match the hand-modeled GLB intersection tiles (GRG custom
  containers: 4way/3way/roundabout — splitter + on/off-ramps with
  junction_prefab_ramps(true)). Unmatched junctions keep the
  procedural n-gon patch; per-junction overrides live in the
  Inspector (Layers > junction row > prefab tile combo).

traffic simulation (3D Scene toolbar > "traffic")
  vehicle agents follow the curves right-hand lane by lane, keep a
  car-following gap and obey per-junction signal phases with stop
  lines at the crosswalks. roadkit-cli --sim 25 renders a snapshot.

junction_info()         list detected junctions (crossing / roundabout)

  Detection channels — like the Godot plugin's joins:
    - endpoints within snap_m -> welded + n-gon patch
    - two roads CROSSING     -> 4-leg n-gon intersection (IntersectionNGon)
    - a road ENDING on another -> T-junction
    - 3+ roads MEETING       -> roundabout ring (custom-container style)

BUNDLED GLB ASSETS (assets/glb/)
--------------------------------
  props          vehicle, tree, streetlight, milestone, junctionMarker
  traffic lights traffic_light_tz (vertical 3-aspect),
                 traffic_light_usa (mast arm), traffic_light_eu (louvres + ped head)
  signs          sign_stop, sign_yield, sign_speed_50, sign_speed_80,
                 sign_no_entry, sign_warning, sign_road_name, sign_pedestrian
  BRT Mwendokasi brt_station (30 m median platform), brt_shelter_small

EXAMPLE
-------
  extends Node3D

  func _ready():
      var curve = RoadCurve.new("My road")
      var a = RoadPoint.new()
      a.position = Vector3(-60, 0, 0)
      a.lanes = 2
      curve.add_point(a)
      var b = RoadPoint.new()
      b.position = Vector3(60, 0, 0)
      b.lanes = 2
      curve.add_point(b)
      curve.build_mesh()
      curve.add_props("streetlight", 25.0, true)
      curve.stamp_prop("vehicle", 0.5, 1.75)
      print("built " + str(curve.length()) + " m of road")
"#
}
