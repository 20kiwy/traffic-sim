# RoadArchitect → Rust port map (docs)

Source of truth: `clone/RoadArchitect` (submodule @ f924b21, Unity C#).
This document maps every RoadArchitect visual system to its Rust
implementation in this workspace, including the **2D position + movement**
semantics of each component (where it sits on the road cross-section and
how it travels/orients along the curve). All units are meters (1 Unity
unit = 1 m), matching our scene units.

Legend: **RA** = RoadArchitect, **RU** = our Rust implementation.

---

## 1. Road mesh

| RA (source) | Value / rule | RU |
|---|---|---|
| `GSDRoad.opt_LaneWidth` (GSDRoad.cs:44) | 5.0 m default, clamp 0.2–500 | `RoadPoint.lane_width` (per-point, GRG-parity 3.5 m TZA default; RA value kept in `tanzania::RA_LANE_WIDTH` reference) |
| `opt_ShoulderWidth` (:47) | 3.0 m | `RoadPoint.shoulder` |
| `opt_RoadDefinition` (:49) | 5.0 m per mesh row | mesh sampling 4 m (`MeshQuality::sample_m`) |
| `opt_MaxGrade` (:55) | 0.08 (8 %) | `curve.auto_orient` grade clamp in `roads::curve` |
| Road row vertex layout (GSDRoadUtilityT.cs:2322) | 4 verts (L,L,R,R dup pairs) | `strip_dyn` quad strips (L/R pairs + UV seams) |
| Road UV V tiling (:5269) | 1 repeat / **5 m** of travel | `v_tile` + `v_snap` per-segment GRG snapping |
| World-mapped uv2 (:4714) | `(x·0.2, z·0.2)` → 5 m | shoulders/gutters use world-space UV /4 m |
| Asphalt material (GSDRoad.cs:1148) | GSDRoad1_Diffuse 1024² @ 4×2 tiling → **2.5 m tile** | slot `TEX_RA_ASPHALT` (converted to RKTX); road body keeps GRG trim sheet, RA asphalt used on shoulders/links/pavement patches |
| Z-fight layering (FixZ :1414) | markers +0.01/+0.02 m | same +0.01/+0.02 offsets in `mesh3d` strips |
| Mesh split at 64 k verts (GSDRoadUtility.cs:1411) | auto re-split | not needed (parts already per-strip) |

**2D position semantics.** RA lays the road cross-section around the spline
tangent `P`: `right = (P.z, 0, −P.x)`; edge offsets are
`RoadWidth/2` (asphalt), `+ShoulderWidth` (shoulder outer), ramp +25 % more
dropped −0.35 m. RU: identical basis — `per = (−dz, dx)/l` left normal in
`build_parts`, offsets from `paved_width()`; the gutter profile
`(2.0, −0.5)` extends past the shoulder like RA's ramp.

---

## 2. Edge objects & extrusions

**Edge objects (placed instances)** — GSDRoadEdgeObjects.cs. Placement
marches the spline every `MeterSep` meters, offset `HorizontalSep` from the
road edge, raised `VerticalRaise`, snapped to terrain (`bMatchTerrain`),
left-side instances rotated 180° yaw (oncoming). Singles use spline t
(0.121 / 0.14585). End gate: 125 m before segment end.

| RA preset | Mesh | MeterSep | HorizSep | RU asset | RU placement |
|---|---|---|---|---|---|
| GSDSignStop | Signs/GSDSignStop | single @0.121 | 0 | `signStop` GLB | at junction stop lines (right-hand corner, yaw facing approach) |
| GSDSign330/396/… FedSigns | FBX | single @0.1459 | 0 | `signSpeed50`/`signSpeed80` GLB | arterial approaches |
| GSDSignRightTurnOnly | prefab | single @0.17 | 5 m | (turn-arrow overlay in plan/3D labels) | on turn-lane approaches |
| GSDStreetLightSingle/Double | prefab | **30 m** | 0 | `streetlight` GLB | every 30–40 m along arterials, alternating sides |
| GSDRoadReflector | FBX | 3 m | 0 | (skip — sub-texel) | — |
| TrafficCone / Barrels | prefabs | 4–5 m | 0 | (workzone pack later) | — |

**Extrusions (continuous splinated meshes)** — GSDRoadMeshSplinate.cs.
Instances chained by projecting the mesh's own length along the tangent
(zero-gap), `bMatchRoadDefinition` rescales each copy to exactly one row
(5 m) and retiles UV to 1 repeat per copy.

| RA extrusion | Profile | RU |
|---|---|---|
| GSDWBeam1L/R (`GSDWBeam2.FBX`) | corrugated W-beam on posts | `mesh3d::guardrail_parts` — W profile (3 corrugations, 0.31 m tall) + blade posts every 4 m, TEX_RA_GALV |
| KRail (`GSDBarrier-KRail`) | concrete jersey trapezoid | `mesh3d::krail_parts` — 0.8 m trapezoid, TEX_RA_CONCRETE, median/median-BRT edges |
| GSDSidewalk | curb+walk slab | existing sidewalk/gutter strips (`build_parts`) |
| GSDRumbleStrip | `VertRaise=0.03` | rumble band inside trim sheet (existing) |
| GSDRailing1–9 | bridge parapets | `railing_parts` (parapet + handrail + posts) on elevated stretches |
| Caps (`KRailCap`, `Sidewalk-Cap`) | end caps | abutment/wingwall pieces at elevated ends (`abutment_parts`) |

**2D position + movement.** Extrusions ride the *edge polylines* (left/right
asphalt edge), always tangent-aligned (`SetTRS(pos, LookRotation(tangent))`
in RA ⇒ RU: per-sample frame `(p, tangent, normal)` in `strip_dyn`/rail
loops); `HorizontalSep ±5 m` flips side. Guardrails on embankments (RA
`WBeamLeftTurn` preset adds a `GSDSign396` every 10 m at +0.4 m).

---

## 3. Bridges

RA bridge kits (Editor/Library/B/W/*.gsd): deck modules 5 m long
(`BridgeBase-19/29/39w-5l-1d` ⇒ 19/29/39 m wide), underside beam grid
(`BridgeBaseGrid`/`SteelBeam`, V=−1.01), railings at **0.72–0.79 m** above
deck, piers every **40–60 m** (`BridgePillar1-5`, V −4…−6), special kits:
semi-arch (`BridgeBase6/7/8-*40M`), suspension (cables every 5 m @±16.5 m).

RU mapping (`roads::mesh3d`, driven by `RoadCurve` layer/elevation):
- deck + underside thickness ⇒ existing surface underside (`underside_thickness`)
- piers ⇒ `pier_parts` (every 26 m — tightened to JICA spans; RA 40–60 m for reference)
- parapet + handrail ⇒ `railing_parts` (0.9 m parapet + 1.15 m handrail)
- abutments + cap beams ⇒ `abutment_parts`
- expansion joints ⇒ `joint_parts` (per span starts from `tanzania::JICA_SPANS`)
- Kijazi/Tazara/BRT flyovers ⇒ `dsm.rs` builders (JICA Fig 5.3.10 geometry:
  300 m box-girder bridge, 137.3/175.4 m approaches, 4 % grade, 7 m crown)

---

## 4. Intersections

RA (GSDRoadIntersection.cs): `ThreeWay/FourWay` × `NoTurnLane/TurnLane/
BothTurnLanes`; approach rebuild zone ±4.5×RoadWidth (5.5 with turn lanes);
surface plane +0.1 m; blend radius 75 m (125 m with turn lanes) ×(1.25/1.35
for 4/6 lanes); corner points = edge-polyline crossings; lane split
fractions [0, 0.5, 1] / 4-pt / 5-pt; **no roundabout type** (RU: WFC
roundabout prefabs are an extension); control StopSign_AllWay /
TrafficLight1 (US mast) / TrafficLight2 (Euro) / None.

Markings: CenterMarkers quad (`GSDInterCenter-{None,Left,Both}-{2L,4L,6L}`),
lane strips `GSDInterWhiteLYellowR` etc. with 5 m UV, TiledExt pavement
(GSDRoad6 world UV) + StretchExt marker.

RU: `roads::junction` (crossing/T-join suppression across layers/2.5 m elev),
`mesh3d::marking_parts_for` (center + lane strips + stop bars + zebra
crossings), `prefab.rs` GLB junction tiles (4way/3way/roundabout/splitter/
onramp/offramp) UV-mapped to the trim sheet, `signals.rs` registry
(Uncontrolled/Signal/Police/Roundabout + JICA presets).

**2D position + movement.** Intersection pieces are anchored at the junction
node: corners at edge crossings, signal poles at `ShoulderWidth·0.45` from
each corner, mast arm reach `LanesForInter·LaneWidth + DistFromCorner`,
signal heads at `LaneWidth·0.5 + i·LaneWidth` along the arm (through), left
head −1.8 m (yield sign −2.5/−3.2), right head +0.95. Movement = the signal
phase state machine (5/2/1/10/2/1 s RA defaults; RU registry:
`GroupTiming{green, yellow, all_red}` per JICA 120/130 s plans), heads
toggle color only — geometry static.

---

## 5. Traffic lights

RA (GSDTrafficLightController.cs): phase cycle `[LT 5, Y 2, AR 1, Main 10,
Y 2, AR 1]×2`; atlas U offsets Red 0.0 / Yellow 0.334 / Green 0.667
(yield head: 0/0.2/0.4/0.6/0.8); heads on masts `GSDInterTLB1/2`; pole
height `7.6·hMod` (hMod = clamp(TLDistance/10·0.7, 1, 20)); street light
option range 30 m intensity 1 color (1, 0.745, 0.275) sodium.

RU: `traffic_sim.rs` light_state/light_poses (pole beside each stop line
facing the approach — Tanzania LHT), `viewport3d::draw_traffic_lights`
(3-lamp heads, live G/A/R, click-select + drag nudge), `signals.rs`
`SignalPlan::state_at` (green/yellow/all-red per group). GLB poles:
`trafficLightTz/Eu/Usa`.

---

## 6. Terrain

RA (GSDTerrain.cs + terraforming): heightmap nodes under each road row
projected to the row plane − 0.01 m (`TerrainSubtract_Match`), slope margin
`MatchHeightsDistance·0.5 = 25 m`, cleared trees/details within 30 m, splat
map paints ±`SplatWidth = 30 m` black/white mask, bridges skip terrain.

RU: `roads::terrain.rs` heightfield + `TerrainFlatten` (approximate
flattening under roads, disabled on the 9 km DSM bbox for memory — flat
grass disc + screen-space grid instead), `scene.rs::build_terrain_ground`.

---

## 7. Signs & props (asset conversion)

RA sign library → RU GLB set (embedded, `library.rs::embedded`):
`signStop` (GSDSignStop), `signYield`, `signSpeed50/80` (FedSigns 330/396
equivalents), `signNoEntry`, `signWarning` (Diamond), `signPedestrian`,
`signRoadName` (Square), `trafficLightTz/Eu/Usa` (GSDInterTLB + heads),
`streetlight` (GSDStreetLightSingle, 30 m cadence), `brtStation/
brtShelterSmall` (BRT), fleet `car_sedan/daladala/brt_bus/truck/moto`
(VISSIM-class stand-ins), `tree`, `milestone`, `junctionMarker`.

Texture conversion: RA PNGs → RKTX raw (build-time python
`assets/textures_src/gen_ra_textures.py`), slots 9+:
`ra_asphalt` (GSDRoad1_Diffuse), `ra_concrete` (GSDConcrete1),
`ra_galv` (GSDGalv1), `ra_wbeam` (GSDWBeam), `ra_railing` (GSDRailing4),
`ra_brick` (GSDBrickRoad), `ra_cobble` (GSDCobblestoneRoad) — see §8
for tile sizes.

---

## 8. Textures (RA → RKTX)

| RKTX slot | RA source | RA tiling | RU usage |
|---|---|---|---|
| `ra_asphalt` | GSDRoad1_Diffuse 1024² | 4×2 (2.5 m tile on 10 m road) | shoulders, links, pavement patches, parking aprons |
| `ra_concrete` | GSDConcrete1_Diffuse | 1×1 | piers/abutments/K-rail/buildings |
| `ra_galv` | GSDGalv1_Diffuse | 1×1 | W-beam corrugations, posts |
| `ra_wbeam` | GSDWBeam_Diffuse | 1×1 | W-beam face |
| `ra_railing` | GSDRailing4_Diffuse 512×1024 | 1×1 | bridge parapet inner face |
| `ra_brick` | GSDBrickRoad_Diffuse | 4×2 | building accents |
| `ra_cobble` | GSDCobblestoneRoad_Diffuse | 4×2 | sidewalks/plazas |
| (procedural) | — | — | windows texture for buildings (`windows.raw`) |

---

## 9. Grid — free vs logic-restricted

RA has no world grid — placement is spline-parametric. RU keeps the
TerraZone-style editor grid overlay (free snapping for the user) while
scene logic can **restrict** placement:
- props/buildings snap to the terrain/ground sampler (y = ground height);
- junction prefabs snap to curve crossings (`roads::junction`);
- WFC tile placement restricted by open-edge matching (`prefab.rs`);
- edge objects respect the 125 m end gate + `terminated` points;
- `simflow` restricts vehicle movement to lane graphs (no free roam).
The viewport grid draws regardless (screen-space overlay), but stamping
tools honor `restricted` mode flags (Tool::Stamp with grid snap on/off).

---

## 10. Camera snapshots → Blender-style view modes

RA ships no renderer modes; RU adds Blender-viewport shading equivalents on
the CPU rasterizer (`roadkit::raster::ShadeMode`):
`Solid` (textured lambert), `Normals` (RGB = world normal), `Wireframe`
(hidden-line depth fill + edges), **`ShadedWire`** (solid + edge overlay),
**`Depth`** (grayscale view-z), **`Quality`** (2× SSAA supersampled Solid).
CLI: `--shade <mode>` on every render path; egui toolbar chips mirror it.

---

## 11. R12 — 100% coverage pass

The remaining RA systems landed; nothing in the visual stack is stubbed any more:

| RA system (source) | RU |
|---|---|
| GSDTunnel-2L/4L/6L + caps + lights (Mesh/RoadObj/Tunnel) | `mesh3d::tunnel_parts` — flag `RoadCurve.tunnel`, portal faces + crown strip lights |
| Suspension kit (cables every 5 m @±16.5 m) | `mesh3d::suspension_parts` — `RoadCurve.bridge_style = Suspension` (Selanda bridge) |
| Semi-arch kit (BridgeBase6/7/8-*40M) | `mesh3d::arch_parts` — `bridge_style = Arch` |
| GSDCableBarrier (-Light/-Pole) | `mesh3d::cable_barrier_parts` — `RoadCurve.cable_barrier` on medians ≥ 2 m |
| TrafficCone / Barrels / GSDAtten (Interactive) | GLB props `cone` / `barrel` / `attenuator` (gen_roadwork.py), stamped in the Igesa BRT Phase-4 workzone |
| GSDConcrete2/3, GSDDirtRoad materials | RKTX slots 17–19 (`ra_concrete2/3`, `ra_dirt`) |
| (parity note) GSDRigidBody physics | intentionally replaced by `simflow` lane-graph movement — RA's rigidbody props are for Unity physics demos, not road generation |

Conversion coverage: **100% of the visual/generation stack** (roads, intersections, bridges incl. special kits, tunnels, extrusions, edge objects, traffic lights, terrain, textures, props). Physics is a deliberate paradigm swap, editor tooling is re-imagined as the egui app + .gd bridge.
