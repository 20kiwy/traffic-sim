//! RoadPoint + RoadCurve — the GRG node model (meters, xz plane).
//!
//! Property names / defaults mirror `godot-road-generator` RoadPoint
//! (`nodes/road_point.gd`): the lane model is a `traffic_dir` list of
//! REVERSE-first / FORWARD-last entries, auto-lane-type assignment,
//! gutter profile, alignment, underside thickness, terminated flag.

use crate::curve;
use crate::surface;
use gdmath::vec2::Vec2;
use gdmath::vec3::Vec3;

/// GRG `LaneDir`: NONE / FORWARD / REVERSE / BOTH. The stored `traffic_dir`
/// of a RoadPoint is the ordered list (REVERSE lanes first, FORWARD last).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneDir {
    None,
    Forward,
    Reverse,
    Both,
}

impl LaneDir {
    pub fn label(&self) -> &'static str {
        match self {
            LaneDir::None => "NONE",
            LaneDir::Forward => "FORWARD",
            LaneDir::Reverse => "REVERSE",
            LaneDir::Both => "BOTH",
        }
    }
    pub fn from_char(c: char) -> Option<Self> {
        match c.to_ascii_uppercase() {
            'F' => Some(LaneDir::Forward),
            'R' => Some(LaneDir::Reverse),
            'N' => Some(LaneDir::None),
            'B' => Some(LaneDir::Both),
            _ => None,
        }
    }
    pub fn as_char(&self) -> char {
        match self {
            LaneDir::Forward => 'F',
            LaneDir::Reverse => 'R',
            LaneDir::None => 'N',
            LaneDir::Both => 'B',
        }
    }
}

/// GRG `TrafficUpdate` — the lane add/remove operations driven by the
/// RoadPoint editor's `+ - || - +` button row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrafficUpdate {
    AddForward,
    AddReverse,
    RemForward,
    RemReverse,
    MoveDividerLeft,
    MoveDividerRight,
}

impl TrafficUpdate {
    /// The exact Godot undo/redo action name for this lane operation.
    pub fn undo_label(&self) -> &'static str {
        match self {
            TrafficUpdate::AddForward => "Add forward lane",
            TrafficUpdate::AddReverse => "Add reverse lane",
            TrafficUpdate::RemForward => "Remove forward lane",
            TrafficUpdate::RemReverse => "Remove reverse lane",
            TrafficUpdate::MoveDividerLeft => "Move divider left",
            TrafficUpdate::MoveDividerRight => "Move divider right",
        }
    }
}

/// GRG `Alignment`: how the cross-section centers itself on the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Geometric,
    Divider,
}

/// Per-lane turn designation at a junction approach — the OSM
/// `turn:lanes` vocabulary (what a lane may do at the next junction).
/// The property panel and the junction overlay render these as
/// left / through / right passages (JICA Table 5.2.1 / 5.3.1 style:
/// each approach lists LT / TH / RT lanes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TurnDir {
    Through,
    Left,
    Right,
    LeftThrough,
    ThroughRight,
    LeftRight,
    LeftThroughRight,
    UTurn,
    None,
}

impl TurnDir {
    /// Short arrow-ish label for overlays (egui-safe ASCII).
    pub fn label(&self) -> &'static str {
        match self {
            TurnDir::Through => "TH",
            TurnDir::Left => "L",
            TurnDir::Right => "R",
            TurnDir::LeftThrough => "L|TH",
            TurnDir::ThroughRight => "TH|R",
            TurnDir::LeftRight => "L|R",
            TurnDir::LeftThroughRight => "L|TH|R",
            TurnDir::UTurn => "UT",
            TurnDir::None => "-",
        }
    }

    /// OSM `turn:lanes` token.
    pub fn token(&self) -> &'static str {
        match self {
            TurnDir::Through => "through",
            TurnDir::Left => "left",
            TurnDir::Right => "right",
            TurnDir::LeftThrough => "left;through",
            TurnDir::ThroughRight => "through;right",
            TurnDir::LeftRight => "left;right",
            TurnDir::LeftThroughRight => "left;through;right",
            TurnDir::UTurn => "u_turn",
            TurnDir::None => "none",
        }
    }

    pub fn from_token(t: &str) -> Self {
        match t.trim().to_ascii_lowercase().as_str() {
            "through" | "straight" | "th" => TurnDir::Through,
            "left" | "l" => TurnDir::Left,
            "right" | "r" => TurnDir::Right,
            "left;through" | "through;left" => TurnDir::LeftThrough,
            "through;right" | "right;through" => TurnDir::ThroughRight,
            "left;right" | "right;left" => TurnDir::LeftRight,
            "left;through;right" | "left;right;through" | "through;left;right" => {
                TurnDir::LeftThroughRight
            }
            "u_turn" | "uturn" | "ut" | "reverse" => TurnDir::UTurn,
            _ => TurnDir::None,
        }
    }
}

/// Parse an OSM `turn:lanes` string ("left|through;right") into one
/// TurnDir per lane. `|` and `\n` separate lanes (OSM uses `|`);
/// `;` inside a lane combines movements.
pub fn parse_turn_lanes(s: &str) -> Vec<TurnDir> {
    s.split(|c| c == '|' || c == '\\')
        .map(TurnDir::from_token)
        .collect()
}

/// Serialize a lane list back to the OSM token string.
pub fn format_turn_lanes(lanes: &[TurnDir]) -> String {
    lanes.iter().map(|t| t.token()).collect::<Vec<_>>().join("|")
}

/// One GRG RoadPoint: position + travel bearing + Bezier handle
/// magnitudes + cross-section (lanes / widths).
#[derive(Clone, Debug)]
pub struct RoadPoint {
    pub name: String,
    /// east (m)
    pub x: f64,
    /// south (m)
    pub z: f64,
    /// elevation (m)
    pub elev: f64,
    /// travel direction, radians: dir = (cos θ, sin θ) in (x, z)
    pub bearing: f64,
    /// handle length pointing backwards (towards the previous point)
    pub prior_mag: f64,
    /// handle length pointing forwards (towards the next point)
    pub next_mag: f64,
    /// TOTAL lane count (REVERSE + FORWARD). GRG stores the `traffic_dir`
    /// array; we store the split here (`rev_lanes` REVERSE entries first,
    /// then FORWARD entries) so all downstream mesh code keeps working.
    pub lanes: usize,
    /// number of REVERSE (oncoming) lanes — the "||" divider sits after
    /// these. 0 = one-way. Some points 2 lanes, some 4 with median.
    pub rev_lanes: usize,
    pub lane_width: f64,
    pub shoulder_l: f64,
    pub shoulder_r: f64,
    /// physical center median width (meters, 0 = painted lines only)
    pub median: f64,
    /// sidewalk / pathway width left & right of the pavement (meters)
    pub pathway_l: f64,
    pub pathway_r: f64,
    pub oneway: bool,
    /// GRG: auto assignment of the `lanes` (UV slice) array
    pub auto_lanes: bool,
    /// GRG: mark this point as a non-edge end (excluded from joins)
    pub terminated: bool,
    /// GRG: generate the procedural road geometry for this point
    pub create_geo: bool,
    /// GRG: flatten terrain under segments touching this point
    pub flatten_terrain: bool,
    /// GRG: (width, depth) of the shoulder gutter
    pub gutter_profile: (f64, f64),
    /// GRG: Geometric (center road on origin) or Divider (lane divider on origin)
    pub alignment: Alignment,
    /// GRG: underside thickness; -1 = use container / manager / none
    pub underside_thickness: f64,
    /// layer-engine visibility (eye toggle in the Scene Tree)
    pub visible: bool,
    /// locked points are not draggable in the plan view (lock toggle)
    pub locked: bool,
    /// dirt/surface mode: mud lane surface with rough gravel shoulders
    pub dirt: bool,
    /// pavement surface id (`roads::surface::*`, default ASPHALT). The
    /// legacy `dirt` flag keeps overriding the visual style; MUD/GRAVEL
    /// surfaces imply unpaved behavior (no concrete gutters, no trim
    /// markings, rough junction patches).
    pub surface: u8,
    /// when true, bearing is auto-computed from the neighbouring points
    /// (chord direction) at build time — straight roads need no trig
    pub auto_bearing: bool,
    /// Per-lane turn designations, FORWARD carriageway, OSM order
    /// (leftmost → rightmost lane). Empty = unrestricted/undrawn.
    pub turn_fwd: String,
    /// Per-lane turn designations, REVERSE carriageway (divider →
    /// outer edge, i.e. that carriageway's own left→right). Empty = none.
    pub turn_rev: String,
    /// OSM `layer`: vertical stacking level for grade separation
    /// (0 = ground, 1 = first flyover deck, -1 = underpass…).
    pub layer: i8,
    /// structured bridge deck: renders piers + blocks at-grade junction
    /// detection across this point (flyover support, JICA §5.2.5/5.3.5).
    pub bridge: bool,
}

impl Default for RoadPoint {
    fn default() -> Self {
        Self {
            name: String::new(),
            x: 0.0,
            z: 0.0,
            elev: 0.0,
            bearing: 0.0,
            prior_mag: 30.0,
            next_mag: 30.0,
            lanes: 2,
            rev_lanes: 1,
            lane_width: 3.5,
            shoulder_l: 0.0,
            shoulder_r: 0.0,
            median: 0.0,
            pathway_l: 0.0,
            pathway_r: 0.0,
            oneway: false,
            auto_lanes: true,
            terminated: false,
            create_geo: true,
            flatten_terrain: true,
            gutter_profile: (2.0, -0.5),
            alignment: Alignment::Geometric,
            underside_thickness: -1.0,
            visible: true,
            locked: false,
            dirt: false,
            surface: surface::ASPHALT,
            auto_bearing: false,
            turn_fwd: String::new(),
            turn_rev: String::new(),
            layer: 0,
            bridge: false,
        }
    }
}

impl RoadPoint {
    /// Parsed FORWARD turn lanes (leftmost → rightmost).
    pub fn turn_fwd_list(&self) -> Vec<TurnDir> {
        parse_turn_lanes(&self.turn_fwd)
    }

    /// Parsed REVERSE turn lanes (divider → outer edge).
    pub fn turn_rev_list(&self) -> Vec<TurnDir> {
        parse_turn_lanes(&self.turn_rev)
    }

    /// True when this point sits on a structured bridge deck / flyover
    /// (explicit flag, nonzero layer, or already elevated well above ground).
    pub fn elevated(&self) -> bool {
        self.bridge || self.layer != 0 || self.elev > 2.5
    }

    /// JICA-style approach cross-section string, e.g. "1L 2TH 1R".
    /// Counts lanes per movement from the turn-lane lists (falls back to
    /// lane counts when no turn data).
    pub fn approach_summary(&self) -> String {
        let count = |ls: &[TurnDir]| -> (usize, usize, usize) {
            let mut lt = 0;
            let mut th = 0;
            let mut rt = 0;
            for t in ls {
                match t {
                    TurnDir::Left | TurnDir::LeftRight | TurnDir::UTurn => lt += 1,
                    TurnDir::Through => th += 1,
                    TurnDir::Right => rt += 1,
                    TurnDir::LeftThrough => {
                        lt += 1;
                        th += 1
                    }
                    TurnDir::ThroughRight => {
                        th += 1;
                        rt += 1
                    }
                    TurnDir::LeftThroughRight => {
                        lt += 1;
                        th += 1;
                        rt += 1
                    }
                    TurnDir::None => {}
                }
            }
            (lt, th, rt)
        };
        let (flt, fth, frt) = count(&self.turn_fwd_list());
        let (rlt, rth, rrt) = count(&self.turn_rev_list());
        if flt + fth + frt + rlt + rth + rrt == 0 {
            format!("{}L", self.lanes)
        } else {
            format!("{flt}L {fth}TH {frt}R / {rlt}L {rth}TH {rrt}R")
        }
    }
}

impl RoadPoint {
    pub fn new(x: f64, z: f64) -> Self {
        Self { x, z, ..Default::default() }
    }

    /* ------------- GRG lane model (traffic_dir) ------------- */

    /// The GRG `traffic_dir` array: REVERSE lanes first, then FORWARD.
    pub fn traffic_dir(&self) -> Vec<LaneDir> {
        let mut v = vec![LaneDir::Reverse; self.rev_lanes.min(self.lanes)];
        while v.len() < self.lanes {
            v.push(LaneDir::Forward);
        }
        v
    }

    /// FORWARD lane count (contiguous from the tail in GRG).
    pub fn fwd_lanes(&self) -> usize {
        self.lanes.saturating_sub(self.rev_lanes)
    }

    /// GRG `update_traffic_dir(op)` — add/remove lanes on either side of
    /// the divider. Refusals mirror GRG (`lane_count > 1`, side must have
    /// a lane). Returns false when the operation is a no-op.
    pub fn update_traffic_dir(&mut self, op: TrafficUpdate) -> bool {
        match op {
            TrafficUpdate::AddForward => {
                self.lanes += 1;
                self.oneway = self.rev_lanes == 0;
                true
            }
            TrafficUpdate::AddReverse => {
                self.lanes += 1;
                self.rev_lanes += 1;
                self.oneway = false;
                true
            }
            TrafficUpdate::RemForward => {
                if self.lanes <= 1 || self.fwd_lanes() == 0 {
                    return false;
                }
                self.lanes -= 1;
                self.oneway = self.rev_lanes == 0;
                true
            }
            TrafficUpdate::RemReverse => {
                if self.lanes <= 1 || self.rev_lanes == 0 {
                    return false;
                }
                self.lanes -= 1;
                self.rev_lanes -= 1;
                true
            }
            TrafficUpdate::MoveDividerLeft | TrafficUpdate::MoveDividerRight => {
                // unimplemented in GRG too (`pass` stub) — keep parity
                false
            }
        }
    }

    /// GRG `assign_lanes()` — auto lane-type names for the trimsheet UV
    /// columns. REVERSE side from the left edge: outermost SLOW, interior
    /// MIDDLE, divider-adjacent FAST; mirrored on the FORWARD side.
    pub fn lane_types(&self) -> Vec<&'static str> {
        let n = self.lanes;
        let rev = self.rev_lanes.min(n);
        let fwd = n - rev;
        let mut out = Vec::with_capacity(n);
        if n == 1 {
            out.push("no_marking");
            return out;
        }
        if rev == 0 || fwd == 0 {
            // one-way: innermost (divider side) no marking, others middle/slow
            for i in 0..n {
                out.push(match i {
                    0 => "no_marking",
                    _ if i == n - 1 => "slow",
                    _ => "middle",
                });
            }
            return out;
        }
        // two-way: rev side (left → divider): SLOW … MIDDLE … FAST
        for i in 0..rev {
            out.push(match i {
                0 => "slow",
                _ if i == rev - 1 => "fast",
                _ => "middle",
            });
        }
        // fwd side (divider → right): FAST … MIDDLE … SLOW
        for i in 0..fwd {
            out.push(match i {
                0 => "fast",
                _ if i == fwd - 1 => "slow",
                _ => "middle",
            });
        }
        out
    }

    /// GRG `copy_settings_from` — the exact key set the Godot "Copy
    /// Settings" button captures (minus the transform pair).
    pub fn copy_settings_from(&mut self, src: &RoadPoint) {
        self.auto_lanes = src.auto_lanes;
        self.lanes = src.lanes;
        self.rev_lanes = src.rev_lanes;
        self.oneway = src.oneway;
        self.lane_width = src.lane_width;
        self.shoulder_l = src.shoulder_l;
        self.shoulder_r = src.shoulder_r;
        self.median = src.median;
        self.pathway_l = src.pathway_l;
        self.pathway_r = src.pathway_r;
        self.gutter_profile = src.gutter_profile;
        self.create_geo = src.create_geo;
        self.flatten_terrain = src.flatten_terrain;
        self.alignment = src.alignment;
        self.underside_thickness = src.underside_thickness;
        self.dirt = src.dirt;
        self.surface = src.surface;
        self.turn_fwd = src.turn_fwd.clone();
        self.turn_rev = src.turn_rev.clone();
        self.layer = src.layer;
        self.bridge = src.bridge;
    }

    /// GRG `Flip RoadPoint`: swap prior/next, shoulders, reverse the
    /// traffic_dir, swap mags, keep all connections (180° re-orientation).
    pub fn flip(&mut self) {
        std::mem::swap(&mut self.prior_mag, &mut self.next_mag);
        std::mem::swap(&mut self.shoulder_l, &mut self.shoulder_r);
        std::mem::swap(&mut self.pathway_l, &mut self.pathway_r);
        // gutter_profile / underside_thickness / surface are orientation-
        // symmetric (no left/right split) — nothing to swap for them.
        std::mem::swap(&mut self.turn_fwd, &mut self.turn_rev);
        self.bearing += std::f64::consts::PI;
    }

    /// Unpaved check: the legacy `dirt` flag OR a MUD/GRAVEL surface.
    /// Unpaved roads carry no concrete gutters and no trim markings.
    pub fn unpaved(&self) -> bool {
        self.dirt || surface::is_unpaved(self.surface)
    }

    /// travel direction unit vector in (x, z)
    pub fn dir(&self) -> (f64, f64) {
        (self.bearing.cos(), self.bearing.sin())
    }

    /// Perpendicular pointing to the RIGHT of travel (driver's right):
    /// right = (-sin θ, cos θ) — for θ=0 (east) this is +z (south).
    pub fn right_dir(&self) -> (f64, f64) {
        (-self.bearing.sin(), self.bearing.cos())
    }

    pub fn position(&self) -> Vec3 {
        Vec3::new(self.x as f32, self.elev as f32, self.z as f32)
    }
}

/// A named chain of RoadPoints (GRG's Road container). Consecutive points
/// are connected by cubic Beziers; `closed` adds the closing segment.
/// Carries the full GRG `RoadContainer` property set (Road Generation /
/// Lanes and AI / Decoration groups).
#[derive(Clone, Debug)]
pub struct RoadCurve {
    pub name: String,
    pub points: Vec<RoadPoint>,
    pub closed: bool,
    /// layer-engine visibility (eye toggle in the Scene Tree)
    pub visible: bool,
    /// UI colour bar / category tag for the layer engine
    pub color: [u8; 3],
    /* ---- GRG RoadContainer: Road Generation ---- */
    /// generate procedural road geometry for this container
    pub create_geo: bool,
    /// loop-cut density; -1 = use the manager value (GRG `effective_density`)
    pub density: f64,
    /// fewer loop cuts while dragging (GRG low-poly preview)
    pub use_lowpoly_preview: bool,
    /// flatten terrain under this container's roads
    pub flatten_terrain: bool,
    /// underside thickness; -1 = manager / none
    pub underside_thickness: f64,
    /* ---- GRG RoadContainer: Lanes and AI ---- */
    /// auto-generate RoadLanes for AI agents
    pub generate_ai_lanes: bool,
    /// group name assigned to generated lanes (manager default "road_lanes")
    pub ai_lane_group: String,
    /// auto-free agents registered on a lane when it exits the tree
    pub auto_free_vehicles: bool,
    /// visualize lanes + directions in the editor
    pub draw_lanes_editor: bool,
    /// visualize lanes + directions at runtime
    pub draw_lanes_game: bool,
    /* ---- GRG RoadContainer: Decoration ---- */
    /// create edge curves (left / center / right Path3Ds)
    pub create_edge_curves: bool,
    /* ---- GRG RoadContainer: Internal data ---- */
    pub debug: bool,
}

impl Default for RoadCurve {
    fn default() -> Self {
        Self {
            name: String::new(),
            points: Vec::new(),
            closed: false,
            visible: true,
            color: [0x3f, 0x8e, 0x5a],
            create_geo: true,
            density: -1.0,
            use_lowpoly_preview: false,
            flatten_terrain: true,
            underside_thickness: -1.0,
            generate_ai_lanes: true,
            ai_lane_group: "road_lanes".into(),
            auto_free_vehicles: true,
            draw_lanes_editor: true,
            draw_lanes_game: false,
            create_edge_curves: false,
            debug: false,
        }
    }
}

impl RoadCurve {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), ..Default::default() }
    }

    /// GRG `effective_density()` — container density or the manager default.
    pub fn effective_density(&self, manager_density: f64) -> f64 {
        if self.density > 0.0 {
            self.density
        } else if manager_density > 0.0 {
            manager_density
        } else {
            4.0
        }
    }

    pub fn add_point(&mut self, p: RoadPoint) {
        self.points.push(p);
    }

    /// Re-compute bearings of every point flagged `auto_bearing` from its
    /// neighbours (GRG-style straight-through alignment):
    /// interior points take the chord direction P[i-1] -> P[i+1],
    /// endpoints point at their single neighbour. Runs after every point
    /// insert / before build so scripts can lay straight roads with zero trig.
    pub fn auto_orient(&mut self) {
        let n = self.points.len();
        if n < 2 {
            return;
        }
        for i in 0..n {
            if !self.points[i].auto_bearing {
                continue;
            }
            let dir = if i == 0 {
                let (a, b) = (&self.points[0], &self.points[1]);
                (b.x - a.x, b.z - a.z)
            } else if i == n - 1 {
                let (a, b) = (&self.points[n - 2], &self.points[n - 1]);
                (b.x - a.x, b.z - a.z)
            } else {
                let (a, b) = (&self.points[i - 1], &self.points[i + 1]);
                (b.x - a.x, b.z - a.z)
            };
            if dir.0.abs() > 1e-9 || dir.1.abs() > 1e-9 {
                self.points[i].bearing = dir.1.atan2(dir.0);
            }
        }
    }

    /// Lanes per direction block (for markings).
    pub fn lanes_per_side(&self) -> usize {
        let n = self.points.first().map(|p| p.lanes).unwrap_or(2);
        if self.points.first().map(|p| p.oneway).unwrap_or(false) {
            n
        } else {
            (n / 2).max(1)
        }
    }

    pub fn is_oneway(&self) -> bool {
        self.points.first().map(|p| p.oneway).unwrap_or(false)
    }

    /// Paved width in meters (lanes + median + shoulders at the first point).
    pub fn paved_width(&self) -> f64 {
        match self.points.first() {
            Some(p) => p.lanes as f64 * p.lane_width + p.median + p.shoulder_l + p.shoulder_r,
            None => 7.0,
        }
    }

    /// Paved width at a specific point index (per-point cross-section —
    /// GRG interpolates between points, so junction legs use the width
    /// of the point touching the junction, not the curve's first point).
    pub fn paved_width_at(&self, index: usize) -> f64 {
        match self.points.get(index) {
            Some(p) => p.lanes as f64 * p.lane_width + p.median + p.shoulder_l + p.shoulder_r,
            None => self.paved_width(),
        }
    }

    /// Highest |layer| on this curve (0 = pure ground road).
    pub fn max_layer(&self) -> i8 {
        self.points.iter().map(|p| p.layer).max().unwrap_or(0)
    }

    /// Any structured bridge deck / flyover segment on this curve
    /// (grade separation — JICA §5.2.5 / §5.3.5 flyover support).
    pub fn has_elevated(&self) -> bool {
        self.points.iter().any(|p| p.bridge || p.layer != 0 || p.elev > 2.5)
    }

    /// Mode (most frequent) surface of this curve's RoadPoints — used to
    /// pick junction patch / roundabout annulus textures (e.g. a CEMENT
    /// BRT corridor yields a cement roundabout ring). Ties resolve to the
    /// lowest surface id; an empty curve is ASPHALT.
    pub fn dominant_surface(&self) -> u8 {
        if self.points.is_empty() {
            return surface::ASPHALT;
        }
        let mut counts = [0usize; surface::NAMES.len()];
        for p in &self.points {
            let s = p.surface as usize;
            if s < counts.len() {
                counts[s] += 1;
            }
        }
        let mut best = 0usize;
        for i in 1..counts.len() {
            if counts[i] > counts[best] {
                best = i;
            }
        }
        best as u8
    }

    /// Total corridor width including pathways (meters).
    pub fn corridor_width(&self) -> f64 {
        match self.points.first() {
            Some(p) => self.paved_width() + p.pathway_l.max(p.pathway_r),
            None => 7.0,
        }
    }

    /// Flattened 3D centerline (meters) at `sample_m` arc-length density.
    pub fn centerline(&self, sample_m: f64) -> Vec<Vec3> {
        let pts = &self.points;
        if pts.is_empty() {
            return Vec::new();
        }
        if pts.len() == 1 {
            return vec![pts[0].position()];
        }
        let mut out: Vec<Vec3> = Vec::new();
        for i in 0..pts.len() - 1 {
            let seg = curve::segment_cubic(&pts[i], &pts[i + 1]);
            let (p, _, _) = curve::flatten_cubic(&seg, sample_m.max(0.5));
            let (ea, eb) = (pts[i].elev, pts[i + 1].elev);
            let span = p.len().max(2);
            for (k, v) in p.iter().enumerate() {
                if !out.is_empty() && k == 0 {
                    continue;
                }
                let t = if span > 1 { k as f64 / (span - 1) as f64 } else { 0.0 };
                out.push(Vec3::new(v.x as f32, (ea + (eb - ea) * t) as f32, v.y as f32));
            }
        }
        if self.closed && pts.len() > 2 {
            let seg = curve::segment_cubic(pts.last().unwrap(), &pts[0]);
            let (p, _, _) = curve::flatten_cubic(&seg, sample_m.max(0.5));
            let (ea, eb) = (pts.last().unwrap().elev, pts[0].elev);
            let span = p.len().max(2);
            for (k, v) in p.iter().enumerate() {
                if k == 0 {
                    continue;
                }
                let t = if span > 1 { k as f64 / (span - 1) as f64 } else { 0.0 };
                out.push(Vec3::new(v.x as f32, (ea + (eb - ea) * t) as f32, v.y as f32));
            }
        }
        if out.is_empty() {
            out.push(pts[0].position());
        }
        out
    }

    /// Cumulative arc length of the centerline (meters).
    pub fn length_m(&self) -> f64 {
        let pts = self.centerline(4.0);
        let mut acc = 0.0f64;
        for i in 1..pts.len() {
            let a = pts[i - 1];
            let b = pts[i];
            acc += ((b.x - a.x) as f64).hypot((b.z - a.z) as f64);
        }
        acc
    }

    /// Cumulative lengths + tangent yaws of the centerline at 4 m density.
    pub fn arc_table(&self) -> (Vec<Vec3>, Vec<f64>, Vec<f64>) {
        let pts = self.centerline(4.0);
        let mut cum = Vec::with_capacity(pts.len());
        let mut yaw = Vec::with_capacity(pts.len());
        cum.push(0.0);
        yaw.push(0.0);
        for i in 1..pts.len() {
            let a = pts[i - 1];
            let b = pts[i];
            let dx = (b.x - a.x) as f64;
            let dz = (b.z - a.z) as f64;
            cum.push(cum[i - 1] + dx.hypot(dz));
            yaw.push(dz.atan2(dx));
        }
        (pts, cum, yaw)
    }

    /// Position + tangent yaw at distance `s` meters along the curve.
    pub fn sample_at(&self, s_m: f64) -> Option<(Vec3, f64)> {
        let (pts, cum, yaw) = self.arc_table();
        if pts.len() < 2 {
            return pts.first().map(|p| (*p, 0.0));
        }
        let total = *cum.last().unwrap();
        let d = s_m.clamp(0.0, total);
        let mut i = 1usize;
        while i < cum.len() - 1 && cum[i] < d {
            i += 1;
        }
        let seg = (cum[i] - cum[i - 1]).max(1e-9);
        let t = ((d - cum[i - 1]) / seg).clamp(0.0, 1.0);
        let p = pts[i - 1].lerp(pts[i], t as f32);
        Some((p, yaw[i]))
    }

    /// Closest arc-distance `s` on the curve to a planar point (meters).
    pub fn closest_s(&self, x: f64, z: f64) -> Option<(f64, f64)> {
        let (pts, cum, _) = self.arc_table();
        let mut best = f64::INFINITY;
        let mut best_s = 0.0;
        for (i, p) in pts.iter().enumerate() {
            let dx = (p.x as f64 - x).hypot(p.z as f64 - z);
            if dx < best {
                best = dx;
                best_s = cum[i];
            }
        }
        if best.is_finite() {
            Some((best_s, best))
        } else {
            None
        }
    }

    /// Planar polyline (for the 2D plan view), meters.
    pub fn plan_polyline(&self, sample_m: f64) -> Vec<Vec2> {
        self.centerline(sample_m)
            .into_iter()
            .map(|p| Vec2::new(p.x as f64, p.z as f64))
            .collect()
    }

    /// Centerline samples with the per-sample HALF paved width (meters),
    /// lerped between the bounding RoadPoints of each segment — the 2D
    /// plan and prop placement use this so tapers show everywhere.
    pub fn width_profile(&self, sample_m: f64) -> Vec<(Vec2, f64)> {
        let n_points = self.points.len();
        if n_points < 2 {
            return self
                .points
                .first()
                .map(|p| vec![(Vec2::new(p.x, p.z), self.paved_width() / 2.0)])
                .unwrap_or_default();
        }
        let seg_count = if self.closed { n_points } else { n_points - 1 };
        let mut out: Vec<(Vec2, f64)> = Vec::new();
        for k in 0..seg_count {
            let a = &self.points[k];
            let b = &self.points[(k + 1) % n_points];
            let seg = curve::segment_cubic(a, b);
            let (sp, sc, seg_len) = curve::flatten_cubic(&seg, sample_m.max(1.0));
            let hw_a = self.paved_width_at(k) / 2.0;
            let hw_b = self.paved_width_at((k + 1) % n_points) / 2.0;
            for (j, p) in sp.iter().enumerate() {
                if j == 0 && !out.is_empty() {
                    continue;
                }
                let t = if seg_len > 1e-9 { (sc[j] / seg_len).clamp(0.0, 1.0) } else { 0.0 };
                out.push((Vec2::new(p.x, p.y), hw_a + (hw_b - hw_a) * t));
            }
        }
        out
    }
}
