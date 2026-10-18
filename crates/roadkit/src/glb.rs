//! Minimal GLB (binary glTF 2.0) parser — no external glTF dependency.
//!
//! Supports what the RoadKit assets (and typical simple glTF exports) use:
//! - GLB container: 12-byte header + JSON chunk + optional BIN chunk
//! - mesh primitives with POSITION / NORMAL / COLOR_0 attributes
//! - indexed (u8/u16/u32) or non-indexed triangles (mode 4)
//! - per-primitive materials (baseColorFactor fallback)
//! - node transforms (translation / rotation / scale / matrix)

use gdmath::vec3::Vec3;
use serde_json::Value;

#[derive(Clone, Debug)]
pub struct GltfPrim {
    pub positions: Vec<Vec3>,
    /// per-vertex rgba 0..1
    pub colors: Vec<[f32; 4]>,
    /// per-vertex uv (TEXCOORD_0) — junction prefabs sample the trim sheet
    pub uvs: Vec<[f32; 2]>,
    /// empty => non-indexed (sequential triangles)
    pub indices: Vec<u32>,
    pub color: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct GltfAsset {
    pub name: String,
    pub prims: Vec<GltfPrim>,
    pub min: Vec3,
    pub max: Vec3,
}

impl GltfAsset {
    pub fn vertex_count(&self) -> usize {
        self.prims.iter().map(|p| p.positions.len()).sum()
    }

    pub fn triangle_count(&self) -> usize {
        self.prims
            .iter()
            .map(|p| {
                if p.indices.is_empty() {
                    p.positions.len() / 3
                } else {
                    p.indices.len() / 3
                }
            })
            .sum()
    }
}

const MAGIC: u32 = 0x4654_6C67; // "glTF"
const CHUNK_JSON: u32 = 0x4E4F_534A; // "JSON"
const CHUNK_BIN: u32 = 0x004E_4942; // "BIN\0"

fn rd_u32(b: &[u8], off: usize) -> Option<u32> {
    if off + 4 > b.len() {
        return None;
    }
    Some(u32::from_le_bytes([b[off], b[off + 1], b[off + 2], b[off + 3]]))
}

pub fn parse_glb(bytes: &[u8], name: &str) -> Result<GltfAsset, String> {
    if rd_u32(bytes, 0) != Some(MAGIC) {
        return Err("not a GLB file".to_string());
    }
    let json_len = rd_u32(bytes, 12).ok_or("truncated header")? as usize;
    let json_type = rd_u32(bytes, 16).ok_or("truncated header")?;
    if json_type != CHUNK_JSON {
        return Err("first chunk is not JSON".to_string());
    }
    if 20 + json_len > bytes.len() {
        return Err("json chunk out of bounds".to_string());
    }
    let json: Value = serde_json::from_slice(&bytes[20..20 + json_len])
        .map_err(|e| format!("json: {e}"))?;

    let bin_off = 20 + json_len;
    let mut bin: &[u8] = &[];
    if bin_off + 8 <= bytes.len() {
        let bin_len = rd_u32(bytes, bin_off).unwrap_or(0) as usize;
        let bin_type = rd_u32(bytes, bin_off + 4).unwrap_or(0);
        if bin_type == CHUNK_BIN && bin_off + 8 + bin_len <= bytes.len() {
            bin = &bytes[bin_off + 8..bin_off + 8 + bin_len];
        }
    }

    let get_view = |idx: usize| -> Option<(usize, usize, Option<usize>)> {
        let v = json.get("bufferViews")?.get(idx)?;
        let off = v.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
        let len = v.get("byteLength")?.as_u64()? as usize;
        let stride = v.get("byteStride").and_then(Value::as_u64).map(|s| s as usize);
        Some((off, len, stride))
    };

    fn acc_meta(acc: &Value) -> Option<(usize, u32, &'static str, usize)> {
        let bv = acc.get("bufferView")?.as_u64()? as usize;
        let ct = acc.get("componentType")?.as_u64()? as u32;
        let ty = match acc.get("type")?.as_str()? {
            "SCALAR" => "SCALAR",
            "VEC2" => "VEC2",
            "VEC3" => "VEC3",
            "VEC4" => "VEC4",
            _ => return None,
        };
        Some((
            bv,
            ct,
            ty,
            acc.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize,
        ))
    }

    fn read_floats(
        bin: &[u8],
        get_view: &dyn Fn(usize) -> Option<(usize, usize, Option<usize>)>,
        bv: usize,
        boff: usize,
        ct: u32,
        comps: usize,
        count: usize,
    ) -> Result<Vec<f32>, String> {
        let (off, _len, stride) = get_view(bv).ok_or("missing bufferView")?;
        let comp_size = match ct {
            5126 => 4usize,
            5121 | 5120 => 1,
            5123 | 5122 => 2,
            _ => return Err("unsupported componentType".to_string()),
        };
        let tight = comp_size * comps;
        let stride = stride.unwrap_or(tight).max(tight);
        let mut out = Vec::with_capacity(count * comps);
        for i in 0..count {
            let base = off + boff + stride * i;
            for c in 0..comps {
                let p = base + c * comp_size;
                if p + comp_size > bin.len() {
                    return Err("buffer out of bounds".to_string());
                }
                let v: f32 = match ct {
                    5126 => f32::from_le_bytes([bin[p], bin[p + 1], bin[p + 2], bin[p + 3]]),
                    5121 => bin[p] as f32 / 255.0,
                    5120 => bin[p] as i8 as f32 / 127.0,
                    5123 => u16::from_le_bytes([bin[p], bin[p + 1]]) as f32 / 65535.0,
                    5122 => i16::from_le_bytes([bin[p], bin[p + 1]]) as f32 / 32767.0,
                    _ => 0.0,
                };
                out.push(v);
            }
        }
        Ok(out)
    }

    fn read_indices(
        bin: &[u8],
        get_view: &dyn Fn(usize) -> Option<(usize, usize, Option<usize>)>,
        bv: usize,
        boff: usize,
        ct: u32,
        count: usize,
    ) -> Result<Vec<u32>, String> {
        let (off, _len, _stride) = get_view(bv).ok_or("missing bufferView")?;
        let size = match ct {
            5125 => 4usize,
            5123 => 2,
            5121 => 1,
            _ => return Err("unsupported index componentType".to_string()),
        };
        let mut out = Vec::with_capacity(count);
        for i in 0..count {
            let p = off + boff + size * i;
            if p + size > bin.len() {
                return Err("index buffer out of bounds".to_string());
            }
            let v: u32 = match ct {
                5125 => u32::from_le_bytes([bin[p], bin[p + 1], bin[p + 2], bin[p + 3]]),
                5123 => u16::from_le_bytes([bin[p], bin[p + 1]]) as u32,
                _ => bin[p] as u32,
            };
            out.push(v);
        }
        Ok(out)
    }

    let meshes = json.get("meshes").and_then(Value::as_array).cloned().unwrap_or_default();
    let materials = json.get("materials").and_then(Value::as_array).cloned().unwrap_or_default();
    let nodes = json.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default();
    let accessors = json.get("accessors").and_then(Value::as_array).cloned().unwrap_or_default();

    // Build local transforms per node (flattened scene graph).
    fn node_matrix(n: &Value) -> gdmath::Mat4 {
        if let Some(m) = n.get("matrix").and_then(Value::as_array) {
            let v: Vec<f32> = m.iter().filter_map(Value::as_f64).map(|f| f as f32).collect();
            if v.len() == 16 {
                // glTF stores column-major; ours is row-major
                return gdmath::Mat4 {
                    m: [
                        v[0], v[4], v[8], v[12], //
                        v[1], v[5], v[9], v[13], //
                        v[2], v[6], v[10], v[14], //
                        v[3], v[7], v[11], v[15],
                    ],
                };
            }
        }
        let t = n.get("translation").and_then(Value::as_array);
        let r = n.get("rotation").and_then(Value::as_array);
        let s = n.get("scale").and_then(Value::as_array);
        let (tx, ty, tz) = t
            .map(|a| {
                (
                    a.first().and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    a.get(1).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    a.get(2).and_then(Value::as_f64).unwrap_or(0.0) as f32,
                )
            })
            .unwrap_or((0.0, 0.0, 0.0));
        let (sx, sy, sz) = s
            .map(|a| {
                (
                    a.first().and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    a.get(1).and_then(Value::as_f64).unwrap_or(1.0) as f32,
                    a.get(2).and_then(Value::as_f64).unwrap_or(1.0) as f32,
                )
            })
            .unwrap_or((1.0, 1.0, 1.0));
        let rot = match r {
            Some(a) if a.len() == 4 => {
                let (qx, qy, qz, qw) = (
                    a[0].as_f64().unwrap_or(0.0) as f32,
                    a[1].as_f64().unwrap_or(0.0) as f32,
                    a[2].as_f64().unwrap_or(0.0) as f32,
                    a[3].as_f64().unwrap_or(1.0) as f32,
                );
                quat_to_mat(qx, qy, qz, qw)
            }
            _ => gdmath::Mat4::IDENTITY,
        };
        gdmath::Mat4::translation(tx, ty, tz)
            .mul(&rot)
            .mul(&gdmath::Mat4::scale(sx, sy, sz))
    }

    // Walk the node graph, collecting (mesh_index, world_matrix) pairs.
    let mut instances: Vec<(usize, gdmath::Mat4)> = Vec::new();
    {
        let scene_nodes: Vec<usize> = json
            .get("scenes")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(|s| s.get("nodes"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(Value::as_u64).map(|v| v as usize).collect())
            .unwrap_or_else(|| (0..nodes.len()).collect());

        fn walk(
            idx: usize,
            parent: &gdmath::Mat4,
            nodes: &[Value],
            out: &mut Vec<(usize, gdmath::Mat4)>,
            depth: usize,
        ) {
            if depth > 16 {
                return;
            }
            let Some(n) = nodes.get(idx) else { return };
            let local = node_matrix(n);
            let world = parent.mul(&local);
            if let Some(mi) = n.get("mesh").and_then(Value::as_u64) {
                out.push((mi as usize, world));
            }
            if let Some(children) = n.get("children").and_then(Value::as_array) {
                for c in children {
                    if let Some(ci) = c.as_u64() {
                        walk(ci as usize, &world, nodes, out, depth + 1);
                    }
                }
            }
        }

        for root in scene_nodes {
            walk(root, &gdmath::Mat4::IDENTITY, &nodes, &mut instances, 0);
        }
    }

    let mut prims: Vec<GltfPrim> = Vec::new();

    let mut push_prim = |prim: &Value, world: &gdmath::Mat4| -> Result<(), String> {
        let attrs = prim.get("attributes").cloned().unwrap_or(Value::Null);
        let pos_acc = attrs.get("POSITION").and_then(Value::as_u64).map(|v| v as usize);
        let col_acc = attrs.get("COLOR_0").and_then(Value::as_u64).map(|v| v as usize);
        let uv_acc = attrs.get("TEXCOORD_0").and_then(Value::as_u64).map(|v| v as usize);
        let idx_acc = prim.get("indices").and_then(Value::as_u64).map(|v| v as usize);

        let Some(pos_acc) = pos_acc else { return Ok(()) };
        if pos_acc >= accessors.len() {
            return Ok(());
        }
        let Some((pbv, pct, pty, pboff)) = acc_meta(&accessors[pos_acc]) else {
            return Ok(());
        };
        if pty != "VEC3" {
            return Ok(());
        }
        let count = accessors[pos_acc].get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
        let pf = read_floats(bin, &get_view, pbv, pboff, pct, 3, count)?;
        let mut positions = Vec::with_capacity(count);
        for c in pf.chunks(3) {
            let p = Vec3::new(c[0], c[1], c[2]);
            positions.push(world.transform_point(p).0);
        }

        // colors
        let mut colors = vec![[1.0f32, 1.0, 1.0, 1.0]; count];
        if let Some(ca) = col_acc {
            if ca < accessors.len() {
                if let Some((cbv, cct, cty, cboff)) = acc_meta(&accessors[ca]) {
                    let comps = if cty == "VEC4" { 4 } else { 3 };
                    if let Ok(cf) = read_floats(bin, &get_view, cbv, cboff, cct, comps, count) {
                        for (i, c) in cf.chunks(comps).enumerate() {
                            if i >= count {
                                break;
                            }
                            colors[i] = [c[0], c[1], c[2], if comps == 4 { c[3] } else { 1.0 }];
                        }
                    }
                }
            }
        }

        // uvs (TEXCOORD_0)
        let mut uvs = vec![[0.0f32, 0.0]; count];
        if let Some(ua) = uv_acc {
            if ua < accessors.len() {
                if let Some((ubv, uct, _uty, uboff)) = acc_meta(&accessors[ua]) {
                    if let Ok(uf) = read_floats(bin, &get_view, ubv, uboff, uct, 2, count) {
                        for (i, c) in uf.chunks(2).enumerate() {
                            if i >= count {
                                break;
                            }
                            uvs[i] = [c[0], c[1]];
                        }
                    }
                }
            }
        }

        // indices
        let mut indices = match idx_acc {
            Some(ia) if ia < accessors.len() => {
                if let Some((ibv, ict, _ity, iboff)) = acc_meta(&accessors[ia]) {
                    let icount = accessors[ia].get("count").and_then(Value::as_u64).unwrap_or(0) as usize;
                    read_indices(bin, &get_view, ibv, iboff, ict, icount).unwrap_or_default()
                } else {
                    vec![]
                }
            }
            _ => vec![],
        };
        if indices.is_empty() && count >= 3 {
            indices = (0..count as u32).collect();
        }

        // material base color fallback
        let mut base = [1.0f32, 1.0, 1.0, 1.0];
        if let Some(mi) = prim.get("material").and_then(Value::as_u64) {
            if let Some(mat) = materials.get(mi as usize) {
                if let Some(pbr) = mat.get("pbrMetallicRoughness") {
                    if let Some(bcf) = pbr.get("baseColorFactor").and_then(Value::as_array) {
                        let v: Vec<f32> = bcf
                            .iter()
                            .filter_map(Value::as_f64)
                            .map(|f| f as f32)
                            .collect();
                        if v.len() == 4 {
                            base = [v[0], v[1], v[2], v[3]];
                        }
                    }
                }
            }
        }

        prims.push(GltfPrim { positions, colors, uvs, indices, color: base });
        Ok(())
    };

    for (mesh_idx, world) in &instances {
        if let Some(mesh) = meshes.get(*mesh_idx) {
            if let Some(prims_json) = mesh.get("primitives").and_then(Value::as_array) {
                for prim in prims_json {
                    push_prim(prim, world)?;
                }
            }
        }
    }
    // fallback: meshes without any node reference
    if instances.is_empty() {
        for mesh in &meshes {
            if let Some(prims_json) = mesh.get("primitives").and_then(Value::as_array) {
                for prim in prims_json {
                    push_prim(prim, &gdmath::Mat4::IDENTITY)?;
                }
            }
        }
    }

    // bounds
    let mut min = Vec3::new(f32::INFINITY, f32::INFINITY, f32::INFINITY);
    let mut max = Vec3::new(f32::NEG_INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in &prims {
        for v in &p.positions {
            min.x = min.x.min(v.x);
            min.y = min.y.min(v.y);
            min.z = min.z.min(v.z);
            max.x = max.x.max(v.x);
            max.y = max.y.max(v.y);
            max.z = max.z.max(v.z);
        }
    }
    if !min.x.is_finite() {
        min = Vec3::ZERO;
        max = Vec3::new(1.0, 1.0, 1.0);
    }

    Ok(GltfAsset { name: name.to_string(), prims, min, max })
}

fn quat_to_mat(x: f32, y: f32, z: f32, w: f32) -> gdmath::Mat4 {
    let xx = x * x;
    let yy = y * y;
    let zz = z * z;
    let xy = x * y;
    let xz = x * z;
    let yz = y * z;
    let wx = w * x;
    let wy = w * y;
    let wz = w * z;
    gdmath::Mat4 {
        m: [
            1.0 - 2.0 * (yy + zz), 2.0 * (xy - wz), 2.0 * (xz + wy), 0.0, //
            2.0 * (xy + wz), 1.0 - 2.0 * (xx + zz), 2.0 * (yz - wx), 0.0, //
            2.0 * (xz - wy), 2.0 * (yz + wx), 1.0 - 2.0 * (xx + yy), 0.0, //
            0.0, 0.0, 0.0, 1.0,
        ],
    }
}
