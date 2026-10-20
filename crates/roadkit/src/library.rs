//! Asset library — loads the manifest.json + GLB files and renders
//! thumbnails with the software rasterizer.

use crate::glb::{parse_glb, GltfAsset};
use crate::raster::Frame;
use gdmath::vec3::Vec3;
use gdmath::Mat4;
use std::collections::HashMap;
use std::path::Path;

pub struct AssetEntry {
    pub id: String,
    pub name: String,
    pub role: String,
    pub mesh: GltfAsset,
    /// rgba8 thumbnail (rendered by the software rasterizer)
    pub thumb: Option<Vec<u8>>,
}

pub struct AssetLibrary {
    pub entries: Vec<AssetEntry>,
    pub index: HashMap<String, usize>,
}

impl AssetLibrary {
    /// Load all GLB assets referenced by `assets_dir/glb/manifest.json`.
    /// Tolerant: returns an empty library if anything is missing.
    pub fn load_dir(assets_dir: &Path) -> AssetLibrary {
        let manifest_path = assets_dir.join("glb").join("manifest.json");
        let Ok(manifest_bytes) = std::fs::read(&manifest_path) else {
            return AssetLibrary { entries: vec![], index: Default::default() };
        };
        let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&manifest_bytes) else {
            return AssetLibrary { entries: vec![], index: Default::default() };
        };
        let mut entries: Vec<AssetEntry> = Vec::new();
        if let Some(assets) = manifest.get("assets").and_then(|a| a.as_array()) {
            for a in assets {
                let Some(id) = a.get("id").and_then(|v| v.as_str()) else {
                    continue;
                };
                let file = a.get("file").and_then(|v| v.as_str()).unwrap_or("");
                let file_name = Path::new(file)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                if file_name.is_empty() {
                    continue;
                }
                let path = assets_dir.join("glb").join(&file_name);
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let name = a
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or(id)
                    .to_string();
                let role = a
                    .get("role")
                    .and_then(|v| v.as_str())
                    .unwrap_or("prop")
                    .to_string();
                match parse_glb(&bytes, &name) {
                    Ok(mut mesh) => {
                        // Junction prefab tiles are UV-mapped to GRG's road
                        // texture with per-carriageway layouts we cannot
                        // reproduce faithfully here — remap every UV into the
                        // PLAIN asphalt column (col 7) so the tiles render as
                        // clean hand-modeled asphalt; RoadKit paints its own
                        // zebra/stop-line markings on top.
                        if role.contains("junction prefab") {
                            let island_r =
                                roads::prefab::by_id(id).map(|d| d.island_r).unwrap_or(0.0);
                            remap_junction_prefab(&mut mesh, island_r);
                        }
                        entries.push(AssetEntry { id: id.to_string(), name, role, mesh, thumb: None });
                    }
                    Err(e) => eprintln!("[roadkit] failed to parse {file_name}: {e}"),
                }
            }
        }
        let mut index = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            index.insert(e.id.clone(), i);
        }
        AssetLibrary { entries, index }
    }

    /// Build a library from in-memory GLB payloads (used by --selftest and
    /// any embedding that ships assets inside the binary).
    pub fn from_bytes(items: &[(&str, &str, &str, &[u8])]) -> AssetLibrary {
        let mut entries = Vec::new();
        for (id, name, role, bytes) in items {
            if let Ok(mesh) = parse_glb(bytes, name) {
                entries.push(AssetEntry {
                    id: id.to_string(),
                    name: name.to_string(),
                    role: role.to_string(),
                    mesh,
                    thumb: None,
                });
            }
        }
        let mut index = HashMap::new();
        for (i, e) in entries.iter().enumerate() {
            index.insert(e.id.clone(), i);
        }
        AssetLibrary { entries, index }
    }

    pub fn get(&self, id: &str) -> Option<&AssetEntry> {
        self.index.get(id).map(|&i| &self.entries[i])
    }

    /// The whole GLB prop set embedded in the binary — the wasm32 browser
    /// build has no filesystem, so the engine preview ships its props inside
    /// the wasm. Mirrors `assets/glb/manifest.json` (keep in sync).
    #[cfg(target_arch = "wasm32")]
    pub fn embedded() -> AssetLibrary {
        const ITEMS: &[(&str, &str, &[u8])] = &[
            ("milestone", "route marker", include_bytes!("../../../assets/glb/milestone.glb")),
            ("streetlight", "furniture", include_bytes!("../../../assets/glb/streetlight.glb")),
            ("tree", "roadside vegetation", include_bytes!("../../../assets/glb/tree.glb")),
            ("palm", "roadside vegetation — coconut palm", include_bytes!("../../../assets/glb/palm.glb")),
            ("vehicle", "traffic prop", include_bytes!("../../../assets/glb/vehicle.glb")),
            ("car_sedan", "JICA fleet — private car", include_bytes!("../../../assets/glb/car_sedan.glb")),
            ("daladala", "JICA fleet — daladala minibus", include_bytes!("../../../assets/glb/daladala.glb")),
            ("brt_bus", "JICA fleet — DART BRT bus", include_bytes!("../../../assets/glb/brt_bus.glb")),
            ("truck", "JICA fleet — heavy truck", include_bytes!("../../../assets/glb/truck.glb")),
            ("moto", "JICA fleet — boda-boda motorcycle", include_bytes!("../../../assets/glb/moto.glb")),
            ("junctionMarker", "junction marker", include_bytes!("../../../assets/glb/junctionMarker.glb")),
            ("trafficLightTz", "traffic signal", include_bytes!("../../../assets/glb/traffic_light_tz.glb")),
            ("trafficLightUsa", "traffic signal", include_bytes!("../../../assets/glb/traffic_light_usa.glb")),
            ("trafficLightEu", "traffic signal", include_bytes!("../../../assets/glb/traffic_light_eu.glb")),
            ("signStop", "traffic sign", include_bytes!("../../../assets/glb/sign_stop.glb")),
            ("signYield", "traffic sign", include_bytes!("../../../assets/glb/sign_yield.glb")),
            ("signSpeed50", "traffic sign", include_bytes!("../../../assets/glb/sign_speed_50.glb")),
            ("signSpeed80", "traffic sign", include_bytes!("../../../assets/glb/sign_speed_80.glb")),
            ("signNoEntry", "traffic sign", include_bytes!("../../../assets/glb/sign_no_entry.glb")),
            ("signWarning", "traffic sign", include_bytes!("../../../assets/glb/sign_warning.glb")),
            ("signRoadName", "traffic sign", include_bytes!("../../../assets/glb/sign_road_name.glb")),
            ("signPedestrian", "traffic sign", include_bytes!("../../../assets/glb/sign_pedestrian.glb")),
            ("cone", "workzone — traffic cone", include_bytes!("../../../assets/glb/cone.glb")),
            ("barrel", "workzone — channelizer drum", include_bytes!("../../../assets/glb/barrel.glb")),
            ("attenuator", "workzone — impact attenuator", include_bytes!("../../../assets/glb/attenuator.glb")),
            ("brtStation", "brt station", include_bytes!("../../../assets/glb/brt_station.glb")),
            ("brtShelterSmall", "brt station", include_bytes!("../../../assets/glb/brt_shelter_small.glb")),
            ("4way_1x1", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/4way_1x1.glb")),
            ("4way_2x2", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/4way_2x2.glb")),
            ("3way_1x1", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/3way_1x1.glb")),
            ("3way_2x2", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/3way_2x2.glb")),
            ("roundabout_1x1", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/roundabout_1x1.glb")),
            ("roundabout_2x2", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/roundabout_2x2.glb")),
            ("splitter_1x1", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/splitter_1x1.glb")),
            ("splitter_2x2", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/splitter_2x2.glb")),
            ("highway_onramp", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/highway_onramp.glb")),
            ("highway_offramp", "junction prefab (WFC tile)", include_bytes!("../../../assets/glb/highway_offramp.glb")),
        ];
        let items: Vec<(&str, &str, &str, &[u8])> = ITEMS
            .iter()
            .map(|(id, role, bytes)| (*id, *id, *role, *bytes))
            .collect();
        let mut lib = AssetLibrary::from_bytes(&items);
        // the same junction-prefab UV remap load_dir applies
        for e in &mut lib.entries {
            if e.role.contains("junction prefab") {
                let island_r = roads::prefab::by_id(&e.id).map(|d| d.island_r).unwrap_or(0.0);
                remap_junction_prefab(&mut e.mesh, island_r);
            }
        }
        lib
    }

    /// Render thumbnails for all assets with the software rasterizer.
    pub fn render_thumbs(&mut self, size: usize) {
        for e in self.entries.iter_mut() {
            e.thumb = Some(render_asset_thumbnail(&e.mesh, size));
        }
    }
}

/// Junction prefab tiles: remap every UV into the PLAIN asphalt column
/// (col 7) so the tiles render as clean hand-modeled asphalt; the
/// roundabout island gets a grass vertex tint so the ring reads.
fn remap_junction_prefab(mesh: &mut GltfAsset, island_r: f64) {
    for prim in &mut mesh.prims {
        for uv in &mut prim.uvs {
            uv[0] = (0.885 + (uv[0].rem_euclid(1.0)) * 0.105).min(0.995);
        }
        if island_r > 0.0 {
            for (v, c) in prim.positions.iter_mut().zip(prim.colors.iter_mut()) {
                let r = (v.x * v.x + v.z * v.z).sqrt();
                if v.y > -0.2 && r < (island_r * 0.98) as f32 {
                    // grass island (plain col ≈ 0.25 grey multiplies this)
                    *c = [1.9, 2.5, 1.55, 1.0];
                } else {
                    *c = [1.55, 1.55, 1.55, 1.0];
                }
            }
        }
    }
}

fn render_asset_thumbnail(asset: &GltfAsset, size: usize) -> Vec<u8> {
    let center = (asset.min + asset.max) * 0.5;
    let radius = ((asset.max - asset.min) * 0.5).len().max(0.2);
    let mut cam = gdmath::OrbitCam::default();
    cam.target = center;
    cam.dist = radius * 2.9;
    cam.yaw = 0.65;
    cam.pitch = 0.42;
    let mut frame = Frame::new(size, size, [26, 26, 26, 255]);
    let view = cam.view();
    let proj = cam.proj(1.0); // square thumbnails
    let light = Vec3::new(0.4, 0.85, 0.35).normalized();
    for prim in &asset.prims {
        frame.draw_mesh(
            &prim.positions,
            &prim.indices,
            Some(&prim.colors),
            None,
            None,
            &Mat4::IDENTITY,
            &view,
            &proj,
            cam.eye(),
            light,
            prim.color,
        );
    }
    frame.color
}
