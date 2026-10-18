//! OSM + Overpass providers.
//!
//! std has no TLS, so all network fetching is a thin, cache-aware wrapper
//! around the system `curl` binary (the accepted pattern in this
//! sandbox): `http_get` shells out to
//! `curl -s -S -m 60 --get -A traffic-sim/0.4 --get <url> -o <tmpfile>`,
//! writing to a temp file inside the target cache directory and then
//! renaming / reading it. Everything above that (bbox fetches, full-way
//! fetches, Overpass mirror rotation) is plain JSON handling plus a small
//! in-memory index over OSM JSON documents.

use crate::types::GeoBounds;
use serde_json::Value;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

const USER_AGENT: &str = "traffic-sim/0.4 (road data import)";

/// Overpass interpreters tried in order until one answers with JSON.
const OVERPASS_MIRRORS: &[&str] = &[
    "https://overpass.private.coffee/api/interpreter",
    "https://overpass-api.de/api/interpreter",
    "https://overpass.kumi.systems/api/interpreter",
    "https://overpass.osm.ch/api/interpreter",
];

/// Unique suffix for temp files (pid + call counter) — shared by the
/// curl download path and the gzip helper.
pub(crate) fn unique_suffix() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!("{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed))
}

/// HTTP GET via the system `curl` binary.
///
/// * If `cache` is given **and the file exists**, its bytes are returned
///   immediately (offline / repeat runs are free).
/// * Otherwise curl downloads to a dot-temp file in the same directory
///   (`cache`) or the system temp dir (`cache == None`) and the bytes are
///   read back. Successful, non-empty bodies are persisted to `cache`
///   when provided. `--compressed` makes curl transparently inflate
///   `Content-Encoding: gzip` responses, so cache files are always plain.
///
/// Fails when curl exits non-zero, the body is empty, or the temp file
/// cannot be read; the temp file is always cleaned up.
pub fn http_get(url: &str, cache: Option<&Path>) -> io::Result<Vec<u8>> {
    if let Some(p) = cache {
        if let Ok(bytes) = std::fs::read(p) {
            return Ok(bytes);
        }
    }

    let (tmp_path, final_path): (PathBuf, Option<PathBuf>) = match cache {
        Some(p) => {
            let dir = p
                .parent()
                .filter(|d| !d.as_os_str().is_empty())
                .unwrap_or_else(|| Path::new("."));
            std::fs::create_dir_all(dir)?;
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("download");
            (dir.join(format!(".{name}.{}.tmp", unique_suffix())), Some(p.to_path_buf()))
        }
        None => {
            let tmp = std::env::temp_dir().join(format!("tsdata-{}.tmp", unique_suffix()));
            (tmp, None)
        }
    };

    let run = Command::new("curl")
        .args([
            "-s", "-S", "-m", "60", "-L", "--get", "--compressed", "-A", USER_AGENT, "-o",
        ])
        .arg(&tmp_path)
        .arg(url)
        .output();

    let out = match run {
        Ok(o) => o,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(io::Error::new(
                e.kind(),
                format!("curl spawn failed for {url}: {e}"),
            ));
        }
    };
    if !out.status.success() {
        let _ = std::fs::remove_file(&tmp_path);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let detail = stderr.trim();
        let detail = if detail.is_empty() {
            format!("exit code {}", out.status)
        } else {
            detail.to_string()
        };
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("curl failed for {url}: {detail}"),
        ));
    }

    let bytes = match std::fs::read(&tmp_path) {
        Ok(b) => b,
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            return Err(e);
        }
    };
    let _ = std::fs::remove_file(&tmp_path);

    if bytes.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            format!("empty response from {url}"),
        ));
    }

    // Persist to the cache path (a failed cache write must not lose the
    // payload we are about to return).
    if let Some(p) = final_path {
        let _ = std::fs::write(p, &bytes);
    }
    Ok(bytes)
}

/// Percent-encode a string for use as a URL query component: everything
/// except unreserved characters (`A-Z a-z 0-9 - . _ ~`) becomes `%XX`.
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for &b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// FNV-1a 64-bit hash — cheap stable cache key for Overpass queries.
fn fnv1a64(s: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in s.as_bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// OSM API `map.json` URL for a bounding box.
///
/// OSM bbox order is **left,bottom,right,top** = `west,south,east,north`.
pub fn osm_bbox_url(b: &GeoBounds) -> String {
    format!(
        "https://api.openstreetmap.org/api/0.6/map.json?bbox={},{},{},{}",
        b.west, b.south, b.east, b.north
    )
}

/// Parse a response body as JSON, rejecting HTML error pages.
fn parse_json(url: &str, bytes: &[u8]) -> io::Result<Value> {
    if bytes.first() == Some(&b'<') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{url}: server returned HTML (error page / rate limit) instead of JSON"),
        ));
    }
    serde_json::from_slice(bytes)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{url}: bad JSON: {e}")))
}

/// Fetch a URL into a JSON value with a file cache. A stale/corrupt cache
/// entry is removed and re-fetched.
fn fetch_json_cached(url: &str, cache: &Path) -> io::Result<Value> {
    if let Ok(bytes) = std::fs::read(cache) {
        match parse_json(url, &bytes) {
            Ok(v) => return Ok(v),
            Err(_) => {
                // stale or HTML error page cached previously → refetch
                let _ = std::fs::remove_file(cache);
            }
        }
    }
    let bytes = http_get(url, Some(cache))?;
    parse_json(url, &bytes)
}

/// Fetch all objects in a bounding box from the OSM 0.6 API.
///
/// Cached as `osm_bbox_{west}_{south}_{east}_{north}.json` inside
/// `cache_dir`.
pub fn fetch_bbox(b: &GeoBounds, cache_dir: &Path) -> io::Result<Value> {
    let cache = cache_dir.join(format!(
        "osm_bbox_{:.5}_{:.5}_{:.5}_{:.5}.json",
        b.west, b.south, b.east, b.north
    ));
    fetch_json_cached(&osm_bbox_url(b), &cache)
}

/// Fetch one way with all its member nodes (`/way/{id}/full.json`).
pub fn fetch_way_full(way_id: i64, cache_dir: &Path) -> io::Result<Value> {
    let url = format!("https://api.openstreetmap.org/api/0.6/way/{way_id}/full.json");
    fetch_json_cached(&url, &cache_dir.join(format!("osm_way_{way_id}.json")))
}

/// Run an Overpass QL query (GET `?data=<percent-encoded query>`),
/// rotating through [`OVERPASS_MIRRORS`] until one returns JSON.
///
/// Responses that are empty or start with `<` (HTML error page) count as
/// a mirror failure. Successful bodies are cached per-query (FNV-1a of
/// the query string) inside `cache_dir`.
pub fn overpass(query: &str, cache_dir: &Path) -> io::Result<Value> {
    let cache = cache_dir.join(format!("overpass_{:016x}.json", fnv1a64(query)));
    if let Ok(bytes) = std::fs::read(&cache) {
        if bytes.first() != Some(&b'<') {
            if let Ok(v) = serde_json::from_slice(&bytes) {
                return Ok(v);
            }
        }
        let _ = std::fs::remove_file(&cache);
    }

    let encoded = percent_encode(query);
    let mut last_err = String::from("no mirrors tried");
    for mirror in OVERPASS_MIRRORS {
        let url = format!("{mirror}?data={encoded}");
        match http_get(&url, None) {
            Ok(bytes) => {
                if bytes.first() == Some(&b'<') {
                    last_err = format!("{mirror}: HTML error page");
                    continue;
                }
                match serde_json::from_slice::<Value>(&bytes) {
                    Ok(v) => {
                        let _ = std::fs::write(&cache, &bytes);
                        return Ok(v);
                    }
                    Err(e) => last_err = format!("{mirror}: bad JSON ({e})"),
                }
            }
            Err(e) => last_err = format!("{mirror}: {e}"),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::Other,
        format!(
            "all {} overpass mirrors failed; last error: {last_err}",
            OVERPASS_MIRRORS.len()
        ),
    ))
}

/// Load a JSON document from local disk (offline assets / cache files).
pub fn load_local(path: &Path) -> io::Result<Value> {
    let bytes = std::fs::read(path)
        .map_err(|e| io::Error::new(e.kind(), format!("read {}: {e}", path.display())))?;
    serde_json::from_slice(&bytes).map_err(|e| {
        io::Error::new(io::ErrorKind::InvalidData, format!("parse {}: {e}", path.display()))
    })
}

/// One OSM way: id, ordered node ids and the raw tag map.
#[derive(Clone, Debug)]
pub struct OsmWay {
    pub id: i64,
    pub nodes: Vec<i64>,
    pub tags: serde_json::Map<String, Value>,
}

impl OsmWay {
    /// String value of a tag, if present.
    pub fn tag(&self, key: &str) -> Option<&str> {
        self.tags.get(key).and_then(|v| v.as_str())
    }

    /// True when the way is a closed loop (first node id == last node id,
    /// and at least 4 entries — OSM convention for areas / roundabouts).
    pub fn is_closed(&self) -> bool {
        self.nodes.len() >= 4 && self.nodes.first() == self.nodes.last()
    }
}

/// In-memory index over an OSM JSON document (`map.json`, Overpass or a
/// merged local asset): node id → (lat, lon) plus every way.
pub struct OsmIndex {
    nodes: HashMap<i64, [f64; 2]>,
    ways: Vec<OsmWay>,
    first_node: Option<(f64, f64)>,
}

impl OsmIndex {
    /// Index a document's `elements` array. Non-node/way elements
    /// (relations) are skipped.
    pub fn new(doc: &Value) -> Self {
        let mut nodes: HashMap<i64, [f64; 2]> = HashMap::new();
        let mut ways: Vec<OsmWay> = Vec::new();
        let mut first_node: Option<(f64, f64)> = None;

        if let Some(elems) = doc.get("elements").and_then(|v| v.as_array()) {
            for e in elems {
                match e.get("type").and_then(|v| v.as_str()).unwrap_or("") {
                    "node" => {
                        let (Some(lat), Some(lon)) = (
                            e.get("lat").and_then(Value::as_f64),
                            e.get("lon").and_then(Value::as_f64),
                        ) else {
                            continue;
                        };
                        if first_node.is_none() {
                            first_node = Some((lat, lon));
                        }
                        if let Some(id) = e.get("id").and_then(Value::as_i64) {
                            nodes.insert(id, [lat, lon]);
                        }
                    }
                    "way" => {
                        let id = e.get("id").and_then(Value::as_i64).unwrap_or(0);
                        let nodes_list = e
                            .get("nodes")
                            .and_then(|v| v.as_array())
                            .map(|a| a.iter().filter_map(|x| x.as_i64()).collect())
                            .unwrap_or_default();
                        let tags = e
                            .get("tags")
                            .and_then(|v| v.as_object())
                            .cloned()
                            .unwrap_or_default();
                        ways.push(OsmWay { id, nodes: nodes_list, tags });
                    }
                    _ => {}
                }
            }
        }
        Self { nodes, ways, first_node }
    }

    /// All ways in document order.
    pub fn ways(&self) -> &[OsmWay] {
        &self.ways
    }

    /// (lat, lon) of a node, if indexed.
    pub fn node_pos(&self, id: i64) -> Option<[f64; 2]> {
        self.nodes.get(&id).copied()
    }

    /// Number of indexed nodes.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// (lat, lon) of the document's FIRST node element — the natural
    /// projection origin for local-meter conversion.
    pub fn first_node(&self) -> Option<(f64, f64)> {
        self.first_node
    }
}
