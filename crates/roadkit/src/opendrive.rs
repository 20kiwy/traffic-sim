//! OpenDRIVE (.xodr) importer — real-world lane-level road networks in,
//! RoadKit `.gd` scripts out.
//!
//! OpenDRIVE is the ASAM standard for lane-level road descriptions
//! (planView line/arc/spiral geometry, elevation profiles, lane sections,
//! signals, junctions) — the format every driving-simulation dataset
//! (Ingolstadt, Highway scenarios, CARLA towns, awesome-openx) ships in.
//!
//! Pipeline (native, wasm-safe):
//!   .xodr → centerline samples (planView geometry + elevationProfile
//!   cubic + laneSection counts/widths) → one `RoadCurve` per road,
//!   emitted as a `.gd` script → the regular RoadKit machinery
//!   (junctions, meshes, plan view, traffic sim) just works.
//!
//! The JVM sibling r:trån (tum-gis/rtron, Apache-2.0) sits ALONGSIDE this
//! module as the CityGML transformer for GIS interoperability — see
//! docs/RTRON.md. Geometry stays native here; rtron covers the
//! OpenDRIVE → CityGML 2.0/3.0 path for 3DCityDB / FME / QGIS.
//!
//! Coordinate mapping (documented in docs/RTRON.md):
//!   OpenDRIVE inertial (x, y, z-up, hdg around +z) → RoadKit world
//!   X = od.x, Z = od.y (planar), Y/elev = od.z (elevationProfile).

use roxmltree::Document;

/// One planView geometry entry: a curve segment starting at s.
#[derive(Debug, Clone)]
pub struct OdGeometry {
    /// start s-coordinate [m] along the road reference line
    pub s: f64,
    /// start pose: inertial x, y [m], heading hdg [rad] (CCW around +z)
    pub x: f64,
    pub y: f64,
    pub hdg: f64,
    /// segment length [m]
    pub length: f64,
    pub kind: OdGeomKind,
}

/// planView geometry type. poly3/paramPoly3 are rejected with a clear
/// message (none of the common datasets in our pipeline use them; add on
/// demand).
#[derive(Debug, Clone)]
pub enum OdGeomKind {
    /// straight segment
    Line,
    /// constant curvature `k` (1/radius; k>0 curves left/CCW)
    Arc(f64),
    /// clothoid: curvature ramps linearly from `curv_start` to `curv_end`
    Spiral(f64, f64),
}

/// cubic elevation profile polynomial: z(ds) = a + b·ds + c·ds² + d·ds³
/// (ds = s − entry.s), per the OpenDRIVE elevationProfile spec.
#[derive(Debug, Clone, Copy, Default)]
pub struct OdPoly3 {
    pub s: f64,
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
}

impl OdPoly3 {
    fn eval(&self, ds: f64) -> f64 {
        self.a + self.b * ds + self.c * ds * ds + self.d * ds * ds * ds
    }
}

/// one laneSection: how many driving lanes each side + the innermost
/// right-lane width (our RoadPoint lanes/lane_width mapping).
#[derive(Debug, Clone, Copy)]
pub struct OdLaneSection {
    pub s: f64,
    pub lanes_left: usize,
    pub lanes_right: usize,
    /// width of the innermost right driving lane [m] (width `a` coefficient)
    pub width_right: f64,
}

impl Default for OdLaneSection {
    fn default() -> Self {
        Self { s: 0.0, lanes_left: 1, lanes_right: 1, width_right: 3.5 }
    }
}

/// one OpenDRIVE `<road>`: reference-line geometry + elevation + lanes.
#[derive(Debug, Clone)]
pub struct OdRoad {
    pub id: i32,
    pub name: String,
    pub length: f64,
    /// RHT / LHT
    pub rule: String,
    pub geoms: Vec<OdGeometry>,
    pub elevs: Vec<OdPoly3>,
    pub sections: Vec<OdLaneSection>,
}

impl OdRoad {
    /// elevation at s (cubic per entry; 0.0 when no profile)
    fn elev_at(&self, s: f64) -> f64 {
        let mut z = 0.0;
        for e in &self.elevs {
            if e.s <= s {
                z = e.eval(s - e.s);
            } else {
                break;
            }
        }
        z
    }

    /// laneSection in effect at s
    fn section_at(&self, s: f64) -> OdLaneSection {
        let mut sec = OdLaneSection::default();
        for c in &self.sections {
            if c.s <= s {
                sec = *c;
            } else {
                break;
            }
        }
        sec
    }
}

/// a parsed OpenDRIVE map: header revision + every `<road>`.
#[derive(Debug, Clone, Default)]
pub struct OpenDriveMap {
    pub rev_major: u32,
    pub rev_minor: u32,
    pub roads: Vec<OdRoad>,
}

impl OpenDriveMap {
    /// Parse an OpenDRIVE dataset (1.4 … 1.8) from XML text.
    pub fn parse(xml: &str) -> Result<Self, String> {
        let doc = Document::parse(xml).map_err(|e| format!("xodr parse: {e}"))?;
        let root = doc.root_element();
        if root.tag_name().name() != "OpenDRIVE" {
            return Err(format!("expected <OpenDRIVE> root, got <{}>", root.tag_name().name()));
        }
        let (mut rev_major, mut rev_minor) = (1u32, 4u32);
        if let Some(h) = root.children().find(|n| n.tag_name().name() == "header") {
            rev_major = h.attribute("revMajor").and_then(|v| v.parse().ok()).unwrap_or(1);
            rev_minor = h.attribute("revMinor").and_then(|v| v.parse().ok()).unwrap_or(4);
        }
        let mut roads = Vec::new();
        for road in root.children().filter(|n| n.tag_name().name() == "road") {
            roads.push(parse_road(road)?);
        }
        if roads.is_empty() {
            return Err("no <road> elements in dataset".into());
        }
        Ok(Self { rev_major, rev_minor, roads })
    }

    pub fn road(&self, id: i32) -> Option<&OdRoad> {
        self.roads.iter().find(|r| r.id == id)
    }

    /// Sample every road's reference line into (x, y, z) triples. The
    /// planView geometry is evaluated analytically (line/arc) or by
    /// midpoint-rule integration (spiral); s is stepped by `step` and every
    /// geometry/elevation/laneSection boundary is kept exactly.
    pub fn sample(&self, step: f64) -> Vec<(i32, Vec<(f64, f64, f64)>)> {
        let step = step.max(1.0);
        self.roads
            .iter()
            .map(|r| {
                let mut ss: Vec<f64> = vec![0.0];
                for g in &r.geoms {
                    push_stepped(&mut ss, g.s, g.s + g.length, step);
                }
                for e in &r.elevs {
                    if e.s > 0.0 {
                        ss.push(e.s);
                    }
                }
                for c in &r.sections {
                    if c.s > 0.0 {
                        ss.push(c.s);
                    }
                }
                if let Some(last) = r.geoms.last() {
                    ss.push((last.s + last.length).min(r.length));
                } else {
                    ss.push(r.length);
                }
                ss.sort_by(|a, b| a.partial_cmp(b).unwrap());
                ss.dedup_by(|a, b| (*a - *b).abs() < 1e-6);
                ss.retain(|s| *s <= r.length + 1e-6);

                let pts = ss
                    .iter()
                    .map(|s| {
                        let (x, y) = plan_view_at(r, *s);
                        (x, y, r.elev_at(*s))
                    })
                    .collect();
                (r.id, pts)
            })
            .collect()
    }

    /// Emit the RoadKit `.gd` script: one RoadCurve per road, RoadPoints
    /// sampled every `step` metres carrying elevation, lane count and lane
    /// width from the dataset. Feed the result through the regular script
    /// runner (CLI --script / app Run / MCP run_script) — junctions, plan
    /// view and traffic sim are handled by the existing machinery.
    pub fn to_gd_script(&self, step: f64) -> String {
        let step = step.max(1.0);
        let mut out = String::with_capacity(64 * 1024);
        out.push_str("# RoadKit — imported from OpenDRIVE (native importer, crates/roadkit/src/opendrive.rs)\n");
        out.push_str(&format!(
            "# source revMajor={} revMinor={} · {} roads · step {:.1} m\n",
            self.rev_major,
            self.rev_minor,
            self.roads.len(),
            step
        ));
        out.push_str("# CityGML interop via r:trån (tum-gis) — see docs/RTRON.md\n");
        out.push_str("extends Node3D\n\n");
        out.push_str("func _ready():\n");

        for road in &self.roads {
            let cname = if road.name.is_empty() {
                format!("Road {}", road.id)
            } else {
                format!("{} ({})", road.name.trim(), road.id)
            };
            out.push_str(&format!("\n        # road id={} length={:.1} m rule={}\n", road.id, road.length, road.rule));
            out.push_str(&format!("        var curve{r} = RoadCurve.new(\"{cname}\")\n", r = road.id, cname = cname.replace('"', "'")));

            let (_, pts) = self
                .sample(step)
                .into_iter()
                .find(|(id, _)| *id == road.id)
                .unwrap_or((road.id, Vec::new()));
            if pts.is_empty() {
                out.push_str("        # (no geometry sampled — skipped)\n");
                continue;
            }

            // lane layout from the first section (per-point sections are a
            // documented TODO — most datasets keep one section per road)
            let sec = road.section_at(0.0);
            let total = sec.lanes_left + sec.lanes_right;
            let oneway = sec.lanes_left == 0 && sec.lanes_right > 0;

            let n = pts.len();
            for (i, (x, y, z)) in pts.iter().enumerate() {
                // spline handle length ≈ 0.45 × local spacing → smooth but
                // faithful to the sampled reference line
                let spacing = if i + 1 < n {
                    let (nx, ny, _) = pts[i + 1];
                    ((nx - x).powi(2) + (ny - y).powi(2)).sqrt()
                } else if i > 0 {
                    let (px, py, _) = pts[i - 1];
                    ((px - x).powi(2) + (py - y).powi(2)).sqrt()
                } else {
                    step
                };
                let mag = (0.45 * spacing).clamp(2.0, 60.0);
                let var = format!("r{}p{}", road.id, i);
                out.push_str(&format!("        var {var} = RoadPoint.new()\n"));
                out.push_str(&format!(
                    "        {var}.position = Vector3({x:.3}, {z:.3}, {y:.3})\n"
                ));
                out.push_str(&format!("        {var}.auto_bearing = true\n"));
                out.push_str(&format!("        {var}.lanes = {total}\n"));
                out.push_str(&format!("        {var}.lane_width = {:.2}\n", sec.width_right.max(1.0)));
                if oneway {
                    out.push_str(&format!("        {var}.oneway = true\n"));
                } else if sec.lanes_left > 0 {
                    out.push_str(&format!("        {var}.rev_lanes = {}\n", sec.lanes_left));
                }
                if i > 0 {
                    out.push_str(&format!("        {var}.prior_mag = {mag:.2}\n"));
                }
                if i + 1 < n {
                    out.push_str(&format!("        {var}.next_mag = {mag:.2}\n"));
                }
                out.push_str(&format!("        curve{rid}.add_point({var})\n", rid = road.id, var = var));
            }
            out.push_str(&format!("        curve{r}.build_mesh()\n", r = road.id));
            out.push_str(&format!(
                "        print(\"{cname}: \" + str(curve{r}.length()) + \" m, {n} points\")\n",
                r = road.id,
                cname = cname.replace('"', "'"),
                n = n
            ));
        }
        out
    }
}

/// push s values from `from` to `to` in ≤ `step` increments (boundaries
/// always land exactly)
fn push_stepped(ss: &mut Vec<f64>, from: f64, to: f64, step: f64) {
    if to <= from {
        return;
    }
    let n = ((to - from) / step).ceil() as usize;
    for k in 1..=n {
        let v = from + (to - from) * (k as f64) / (n as f64);
        ss.push(v.min(to));
    }
}

fn parse_road(road: roxmltree::Node<'_, '_>) -> Result<OdRoad, String> {
    let id: i32 = road
        .attribute("id")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| "road missing id".to_string())?;
    let length: f64 = road
        .attribute("length")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("road {id} missing length"))?;
    let name = road.attribute("name").unwrap_or("").to_string();
    let rule = road.attribute("rule").unwrap_or("RHT").to_string();

    let mut geoms = Vec::new();
    if let Some(pv) = road.children().find(|n| n.tag_name().name() == "planView") {
        for g in pv.children().filter(|n| n.tag_name().name() == "geometry") {
            let s: f64 = g.attribute("s").and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let x: f64 = g.attribute("x").and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let y: f64 = g.attribute("y").and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let hdg: f64 = g.attribute("hdg").and_then(|v| v.parse().ok()).unwrap_or(0.0);
            let length: f64 = g
                .attribute("length")
                .and_then(|v| v.parse().ok())
                .ok_or_else(|| format!("road {id} geometry missing length"))?;
            let kind = match g.children().find(|n| n.is_element()) {
                Some(seg) => match seg.tag_name().name() {
                    "line" => OdGeomKind::Line,
                    "arc" => {
                        let k: f64 = seg
                            .attribute("curvature")
                            .and_then(|v| v.parse().ok())
                            .ok_or_else(|| format!("road {id} arc missing curvature"))?;
                        OdGeomKind::Arc(k)
                    }
                    "spiral" => {
                        let k0: f64 = seg
                            .attribute("curvStart")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0);
                        let k1: f64 = seg
                            .attribute("curvEnd")
                            .and_then(|v| v.parse().ok())
                            .unwrap_or(0.0);
                        OdGeomKind::Spiral(k0, k1)
                    }
                    other => {
                        return Err(format!(
                            "road {id}: planView geometry <{other}> not supported yet (line/arc/spiral only)"
                        ))
                    }
                },
                None => return Err(format!("road {id}: <geometry> without a segment element")),
            };
            geoms.push(OdGeometry { s, x, y, hdg, length, kind });
        }
    }
    if geoms.is_empty() {
        return Err(format!("road {id} has no planView geometry"));
    }

    let mut elevs = Vec::new();
    if let Some(ep) = road.children().find(|n| n.tag_name().name() == "elevationProfile") {
        for e in ep.children().filter(|n| n.tag_name().name() == "elevation") {
            elevs.push(OdPoly3 {
                s: e.attribute("s").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                a: e.attribute("a").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                b: e.attribute("b").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                c: e.attribute("c").and_then(|v| v.parse().ok()).unwrap_or(0.0),
                d: e.attribute("d").and_then(|v| v.parse().ok()).unwrap_or(0.0),
            });
        }
    }
    elevs.sort_by(|a, b| a.s.partial_cmp(&b.s).unwrap());

    let mut sections = Vec::new();
    if let Some(ln) = road.children().find(|n| n.tag_name().name() == "lanes") {
        for ls in ln.children().filter(|n| n.tag_name().name() == "laneSection") {
        let s: f64 = ls.attribute("s").and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let count = |side: &str| -> (usize, f64) {
            let Some(el) = ls.children().find(|n| n.tag_name().name() == side) else {
                return (0, 0.0);
            };
            let lanes: Vec<roxmltree::Node> = el
                .children()
                .filter(|n| n.tag_name().name() == "lane")
                .collect();
            // innermost = smallest |id|
            let mut innermost: Option<(i32, f64)> = None;
            let mut driving = 0usize;
            for l in &lanes {
                let lid: i32 = l.attribute("id").and_then(|v| v.parse().ok()).unwrap_or(0);
                let ty = l.attribute("type").unwrap_or("driving");
                if ty == "driving" || ty == "special1" || ty == "exit" || ty == "entry" {
                    driving += 1;
                }
                let w = l
                    .children()
                    .find(|n| n.tag_name().name() == "width")
                    .and_then(|wn| wn.attribute("a"))
                    .and_then(|v| v.parse::<f64>().ok());
                if let Some(a) = w {
                    if innermost.map(|(i, _)| lid.abs() < i.abs()).unwrap_or(true) {
                        innermost = Some((lid, a));
                    }
                }
            }
            // fall back to any-lane count when types are missing
            let n = if driving > 0 { driving } else { lanes.len() };
            (n, innermost.map(|(_, a)| a).unwrap_or(0.0))
        };
        let (lanes_left, wl) = count("left");
        let (lanes_right, wr) = count("right");
        sections.push(OdLaneSection {
            s,
            lanes_left,
            lanes_right,
            width_right: if wr > 0.0 { wr } else { wl.max(3.5) },
        });
        }
    }
    sections.sort_by(|a, b| a.s.partial_cmp(&b.s).unwrap());

    Ok(OdRoad { id, name, length, rule, geoms, elevs, sections })
}

/// pose (x, y) on the reference line at s: analytic for line/arc,
/// midpoint-rule integration for spirals (clothoids).
fn plan_view_at(road: &OdRoad, s: f64) -> (f64, f64) {
    // the geometry segment containing s
    let Some(g) = road
        .geoms
        .iter()
        .filter(|g| g.s <= s + 1e-9)
        .last()
        .or_else(|| road.geoms.first())
    else {
        return (0.0, 0.0);
    };
    let t = (s - g.s).clamp(0.0, g.length.max(0.0));
    match g.kind {
        OdGeomKind::Line => (
            g.x + t * g.hdg.cos(),
            g.y + t * g.hdg.sin(),
        ),
        OdGeomKind::Arc(k) => {
            if k.abs() < 1e-9 {
                return (g.x + t * g.hdg.cos(), g.y + t * g.hdg.sin());
            }
            let th = g.hdg + k * t;
            let cx = g.x - g.hdg.sin() / k;
            let cy = g.y + g.hdg.cos() / k;
            (cx + th.sin() / k, cy - th.cos() / k)
        }
        OdGeomKind::Spiral(k0, k1) => {
            // heading(h) = hdg + k0·h + (k1−k0)·h²/(2L); integrate position
            let l = g.length.max(1e-6);
            let sub = 64;
            let dh = t / sub as f64;
            let (mut x, mut y) = (g.x, g.y);
            for i in 0..sub {
                let h_mid = (i as f64 + 0.5) * dh;
                let k_mid = k0 + (k1 - k0) * h_mid / l;
                let th = g.hdg + k0 * h_mid + (k1 - k0) * h_mid * h_mid / (2.0 * l);
                x += th.cos() * dh;
                y += th.sin() * dh;
                let _ = k_mid;
            }
            (x, y)
        }
    }
}

/// Convenience: raw .xodr text → RoadKit .gd script text.
pub fn xodr_to_gd_script(xml: &str, step: f64) -> Result<String, String> {
    Ok(OpenDriveMap::parse(xml)?.to_gd_script(step))
}
