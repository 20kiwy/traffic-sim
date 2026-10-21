# r:trån + OpenDRIVE — the data-driven road pipeline

*r:trån* (tum-gis/rtron, Apache-2.0, Kotlin/JVM) is TUM's road-space-model
transformer for **OpenDRIVE ↔ CityGML**. It is integrated here two ways:

1. **Native geometry path (default, no JVM)** — `crates/roadkit/src/opendrive.rs`
   reads OpenDRIVE datasets directly and emits RoadKit `.gd` scripts.
2. **GIS interoperability path** — the rtron jar (verified working) converts
   OpenDRIVE → CityGML 2.0/3.0 for 3DCityDB, FME and QGIS.

The submodule lives at `clone/rtron` (7th reference project next to
RoadArchitect, godot-road-generator, Traffic-Toolkit-DOTS, Mapkit,
egregoria, TerraZone).

## Why OpenDRIVE

OpenDRIVE (ASAM) is THE lane-level road format: planView geometry
(line/arc/spiral), elevationProfile cubic polynomials, lane sections with
widths and types, signals, junctions. Every serious driving dataset ships
it (Ingolstadt/3D Mapping Solutions, CARLA towns, awesome-openx lists).
Importing it = any of those cities becomes a RoadKit scene — no hardcoding.

## Native importer (Rust, wasm-safe)

```
.xodr ──roadkit::opendrive──▶ centerline samples ──▶ RoadKit .gd script
        planView line/arc/spiral          one RoadCurve per road
        elevationProfile (cubic)          RoadPoints with elev/lanes/width
        laneSections (counts/widths)      auto-join → junctions → meshes
```

* lines + arcs are evaluated analytically; spirals (clothoids) by
  64-step midpoint integration of the linear-curvature heading profile
* every geometry/elevation/laneSection boundary is sampled exactly;
  default step 15 m (`--step`)
* lane mapping: driving-lane count left+right → `lanes`, innermost right
  width → `lane_width`, left count → `rev_lanes` (or `oneway` when left=0)
* the generated script goes through the REGULAR pipeline — junction
  detection (`auto-join`), roundabouts, meshes, plan view, traffic sim,
  undo history — with zero new scene plumbing
* coordinate mapping: OpenDRIVE (x, y, z-up) → RoadKit world X = x,
  Z = y, elev = z. (If you overlay a north-up basemap later, mirror Z.)

### CLI

```bash
roadkit-cli --opendrive dataset.xodr --view 3d --out road.png [--step 15] \
            [--emit-script out.gd] [--size WxH] [--focus x,z,r] [--sim S]
```

### MCP (agent / CI)

```json
{"method":"tools/call","params":{"name":"load_opendrive",
 "arguments":{"path":"dataset.xodr","step":15}}}
```

then the usual `render_3d` / `render_plan` / `simulate` tools operate on
the imported scene.

### Verified

| dataset | result |
|---|---|
| `basic_sample_01.xodr` (OpenDRIVE 1.7, 1 road, 200 m line) | 15-point 2-lane road, centerline markings ✓ |
| `basic_sample_02.xodr` (4 roads, 4 arcs, 572 m) | closed ring from arcs, **4 auto-joined junctions**, 44 points ✓ (3D + plan renders) |

## r:trån (JVM stage)

Prebuilt jar from the releases page (needs JDK 11+, sandbox has OpenJDK 21):

```bash
curl -L -o rtron.jar https://github.com/tum-gis/rtron/releases/download/v1.3.0/rtron-1.3.0.jar

# validate a dataset (XSD + semantic checks → reports/)
java -jar rtron.jar validate-opendrive ./input-opendrive ./output-reports

# OpenDRIVE → CityGML 2.0 AND 3.0 (Transportation module: Road,
# TrafficArea, AuxiliaryTrafficArea, Marking) for 3DCityDB / FME / QGIS
java -jar rtron.jar opendrive-to-citygml ./input-opendrive ./output-citygml
```

Verified on the bundled samples: sample 01 → CityGML 3.0 model ≈ 8 MB,
104 warnings / **0 errors**; sample 02 → **0 warnings, 0 errors**.

### When to use which path

| need | path |
|---|---|
| drive the RoadKit editor/sim from real data | native importer |
| publish roads into a GIS stack (3DCityDB, Cesium, QGIS) | rtron → CityGML |
| validate a third-party dataset before import | `validate-opendrive` |
| inspect lane-level semantics (the road space model papers) | rtron sources in `clone/rtron` |

## Roadmap

- [ ] per-laneSection RoadPoints (variable lane counts along one road)
- [ ] `<junction>` connectivity → real RoadKit junctions (contact points)
- [ ] lane markings (RoadMark) → trim-sheet UV columns
- [ ] signals/objects → SignalRegistry entries + GLB props
- [ ] `projString` (PROJ) offset application for geo-referenced datasets
- [ ] rtron CityGML output → Relief (TIN) terrain + Building LOD1/LOD2
      import (complements the DSM pipeline)
