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
                        // zebra/stop-line markings on top. The roundabout
                        // island gets a grass vertex tint so the ring reads.
                        if role.contains("junction prefab") {
                            let def = roads::prefab::by_id(id);
                            let island_r = def.map(|d| d.island_r).unwrap_or(0.0);
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

    /// Render thumbnails for all assets with the software rasterizer.
    pub fn render_thumbs(&mut self, size: usize) {
        for e in self.entries.iter_mut() {
            e.thumb = Some(render_asset_thumbnail(&e.mesh, size));
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
