//! Diagnostic helpers for the integrator CLI — human-readable reports
//! over the raw data assets this crate consumes.

use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Per-layer summary accumulated while describing a tile.
#[derive(Default)]
struct LayerStat {
    count: usize,
    classes: BTreeMap<String, usize>,
    structures: BTreeMap<String, usize>,
    names: Vec<String>,
    brt: usize,
}

/// Parse `..._{z}_{x}_{y}` from a tile file name, if present.
fn tile_zxy_from_name(path: &Path) -> Option<(u32, u32, u32)> {
    let stem = path.file_stem()?.to_str()?;
    let parts: Vec<&str> = stem.split('_').collect();
    if parts.len() < 3 {
        return None;
    }
    let z = parts[parts.len() - 3].parse().ok()?;
    let x = parts[parts.len() - 2].parse().ok()?;
    let y = parts[parts.len() - 1].parse().ok()?;
    Some((z, x, y))
}

/// Describe a Mapbox Vector Tile file: layers, feature counts, class /
/// structure histograms on the road-ish layers, a few sample names, and
/// the lon/lat of the first road vertex when the tile's z/x/y can be
/// recovered from the file name (e.g. `streets_15_19952_17003.mvt`).
pub fn describe_mvt(path: &Path) -> String {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => return format!("{}: read error: {e}", path.display()),
    };
    let features = match crate::mapbox::decode_mvt(&bytes) {
        Ok(f) => f,
        Err(e) => {
            return format!(
                "{} ({} bytes): DECODE ERROR: {e}",
                path.display(),
                bytes.len()
            )
        }
    };

    let mut layers: BTreeMap<String, LayerStat> = BTreeMap::new();
    for f in &features {
        let st = layers.entry(f.layer.clone()).or_default();
        st.count += 1;
        if let Some(c) = f.tags.get("class").and_then(Value::as_str) {
            *st.classes.entry(c.to_string()).or_insert(0) += 1;
        }
        if let Some(s) = f.tags.get("structure").and_then(Value::as_str) {
            *st.structures.entry(s.to_string()).or_insert(0) += 1;
        }
        if let Some(n) = f.tags.get("name").and_then(Value::as_str) {
            let owned = n.to_string();
            if st.names.len() < 5 && !st.names.contains(&owned) {
                st.names.push(owned);
            }
        }
        if f.tags.keys().any(|k| k.to_ascii_lowercase().contains("bus")) {
            st.brt += 1;
        }
    }

    let mut out = String::new();
    out.push_str(&format!(
        "{} ({} bytes): {} layers, {} features\n",
        path.display(),
        bytes.len(),
        layers.len(),
        features.len()
    ));
    for (name, st) in &layers {
        out.push_str(&format!("  layer {name:?}: {} features", st.count));
        if !st.classes.is_empty() {
            let cs: Vec<String> =
                st.classes.iter().map(|(c, n)| format!("{c}({n})")).collect();
            out.push_str(&format!(" | classes: {}", cs.join(" ")));
        }
        if !st.structures.is_empty() {
            let ss: Vec<String> =
                st.structures.iter().map(|(s, n)| format!("{s}({n})")).collect();
            out.push_str(&format!(" | structures: {}", ss.join(" ")));
        }
        if st.brt > 0 {
            out.push_str(&format!(" | brt-tagged: {}", st.brt));
        }
        if !st.names.is_empty() {
            let ns: Vec<String> = st.names.iter().map(|n| format!("\"{n}\"")).collect();
            out.push_str(&format!(" | names: {}", ns.join(", ")));
        }
        out.push('\n');
    }

    if let Some((z, x, y)) = tile_zxy_from_name(path) {
        let focus = features
            .iter()
            .find(|f| f.layer == "road")
            .or_else(|| features.first());
        if let Some(f) = focus {
            if let Some(c) = f.coords.first().and_then(|ring| ring.first()) {
                let ll = crate::mapbox::normalized_to_lonlat(z, x, y, c[0], c[1]);
                out.push_str(&format!(
                    "  tile z{z} {x}/{y}; first {:?} feature vertex -> lon/lat [{:.6}, {:.6}]\n",
                    f.layer, ll[0], ll[1]
                ));
            }
        }
    }
    out
}
