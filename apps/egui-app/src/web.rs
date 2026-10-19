//! WASM entry: eframe `WebRunner` booting the SAME `RoadkitApp` in the
//! browser (WebGL2 via glow). This is the engine preview the download page
//! embeds — identical script bridge, scene pipeline and 3D viewport as the
//! native editor, zero code duplication.

use crate::state::{LaunchConfig, RoadkitApp};

#[cfg(target_arch = "wasm32")]
use eframe::wasm_bindgen::{self, prelude::*};

/// Boot the editor on a canvas. Called from `index.html` after the
/// wasm-bindgen glue instantiates the module:
/// `import init, { start } from './egui_app.js'; await init(); await start('canvas');`
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn start(canvas_id: String) -> Result<(), wasm_bindgen::JsValue> {
    console_error_panic_hook::set_once();
    let cfg = LaunchConfig::default();
    let web_options = eframe::WebOptions::default();
    eframe::WebRunner::new()
        .start(
            &canvas_id,
            web_options,
            Box::new(move |cc| Ok(Box::new(RoadkitApp::with_config(cc, cfg.clone())))),
        )
        .await
}
