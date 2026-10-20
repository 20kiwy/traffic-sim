//! WASM entry: eframe `WebRunner` booting the SAME `RoadkitApp` in the
//! browser (WebGL2 via glow). This is the engine preview the download page
//! embeds — identical script bridge, scene pipeline and 3D viewport as the
//! native editor, zero code duplication.
//!
//! Deep links (R11): `?scene=<id>` boots straight into a Dar es Salaam
//! scenario — demo | dsm (full corridor) | kijazi | tazara | igesa |
//! gerezani | mwenge | selanda. Optional `&radius=<m>` sets the opening
//! camera distance. The `index.html` launcher writes the query string and
//! calls `start()` again without a reload.

use crate::state::{LaunchConfig, RoadkitApp};

#[cfg(target_arch = "wasm32")]
use eframe::wasm_bindgen::{self, prelude::*};

/// JICA Fig 5.5.1 site anchors (scene meters — see roadkit::dsm).
#[cfg(target_arch = "wasm32")]
fn site_coords(id: &str) -> Option<(f64, f64)> {
    Some(match id {
        "kijazi" | "ubungo" => (0.0, 0.0),
        "tazara" => (3261.0, 8647.0),
        "igesa" | "sinza" => (1680.0, -1592.0),
        "gerezani" | "bandari" => (4418.0, 6303.0),
        "mwenge" => (2907.0, -1912.0),
        "selanda" => (8735.0, 8393.0),
        _ => return None,
    })
}

/// Read the current URL query (no reload needed — the launcher uses
/// history.replaceState then calls start() again).
#[cfg(target_arch = "wasm32")]
fn query_pairs() -> Vec<(String, String)> {
    let mut out = Vec::new();
    if let Some(window) = web_sys::window() {
        let search = window.location().search().unwrap_or_default();
        for pair in search.trim_start_matches('?').split('&') {
            if pair.is_empty() {
                continue;
            }
            let mut kv = pair.splitn(2, '=');
            let k = kv.next().unwrap_or("").to_string();
            let v = kv.next().unwrap_or("").to_string();
            out.push((k, v));
        }
    }
    out
}

/// Boot the editor on a canvas. Called from `index.html` after the
/// wasm-bindgen glue instantiates the module:
/// `import init, { start } from './egui_app.js'; await init(); await start('canvas');`
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    let mut cfg = LaunchConfig::default();
    for (k, v) in query_pairs() {
        match k.as_str() {
            "scene" => match v.as_str() {
                "dsm" | "full" | "dar" => cfg.dsm = true,
                other => {
                    if let Some((sx, sz)) = site_coords(other) {
                        cfg.dsm_site = Some((sx, sz));
                    }
                }
            },
            "radius" => {
                if let Ok(r) = v.parse::<f32>() {
                    cfg.dsm_radius = r.clamp(40.0, 3000.0);
                }
            }
            _ => {}
        }
    }
    let web_options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(
            &canvas_id,
            web_options,
            Box::new(move |cc| Ok(Box::new(RoadkitApp::with_config(cc, cfg.clone())))),
        )
        .await
}
