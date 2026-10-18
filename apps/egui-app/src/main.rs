//! RoadKit Godot — pure-Rust road design studio (egui/eframe).
//!
//! Godot-Road-Generator architecture, rebuilt in Rust:
//! - roads are PROCEDURAL (RoadPoint magnitudes → cubic → swept mesh)
//! - GLB assets are ONLY roadside props, joined along curves
//! - .gd scripts (parsed + interpreted by the `gdscript` crate) drive it

use egui_app::{selftest, state::{LaunchConfig, RoadkitApp, Tab}};

mod mcp;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--selftest") {
        let code = selftest::run();
        std::process::exit(code);
    }
    if args.iter().any(|a| a == "--uitest") {
        // headless UI audit: every registered button/action runs + diffs state
        std::process::exit(mcp::uitest());
    }
    if args.iter().any(|a| a == "--mcp") {
        // MCP (Model Context Protocol) server over stdio — artifact-style
        // automated testing of every tool, script, render and button.
        std::process::exit(mcp::serve());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("RoadKit Godot — Godot-Road-Generator-style road studio in Rust");
        println!();
        println!("USAGE");
        println!("  roadkit-godot            launch the editor UI");
        println!("  roadkit-godot --selftest headless end-to-end verification");
        println!("  roadkit-godot --uitest   run EVERY registered UI action headlessly + report");
        println!("  roadkit-godot --mcp      MCP testing server (JSON-RPC 2.0 over stdio):");
        println!("                           run_script, get_scene, render_3d/plan,");
        println!("                           list_actions, do_action, get_assets");
        println!("  roadkit-godot [--script file.gd] [--tab plan|3d]");
        println!("                           preload a script and/or start tab");
        return Ok(());
    }

    // CLI presets (used by the Xvfb image tests)
    let mut cfg = LaunchConfig::default();
    if let Some(i) = args.iter().position(|a| a == "--script") {
        if let Some(path) = args.get(i + 1) {
            cfg.script = Some(path.clone());
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--tab") {
        match args.get(i + 1).map(|s| s.as_str()) {
            Some("plan") => cfg.tab = Some(Tab::Plan),
            Some("3d") => cfg.tab = Some(Tab::Scene3d),
            _ => {}
        }
    }
    if let Some(i) = args.iter().position(|a| a == "--panel") {
        use egui_app::state::DockTab;
        cfg.dock_tab = match args.get(i + 1).map(|s| s.as_str()) {
            Some("layers") => Some(DockTab::Layers),
            Some("properties") => Some(DockTab::Properties),
            Some("assets") => Some(DockTab::Assets),
            Some("script") => Some(DockTab::Script),
            Some("console") => Some(DockTab::Console),
            Some("history") => Some(DockTab::History),
            _ => None,
        };
    }

    let icon = app_icon();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1680.0, 960.0])
            .with_min_inner_size([1024.0, 640.0])
            .with_title("RoadKit Godot — procedural roads · GLB props · .gd scripts")
            .with_icon(egui::IconData {
                width: icon.0,
                height: icon.1,
                rgba: icon.2,
            }),
        ..Default::default()
    };
    eframe::run_native(
        "RoadKit Godot",
        options,
        Box::new(move |cc| Ok(Box::new(RoadkitApp::with_config(cc, cfg.clone())))),
    )
}

/// Programmatic 32×32 icon: dark tile + orange road ribbon + white dashes.
fn app_icon() -> (u32, u32, Vec<u8>) {
    const S: usize = 32;
    let mut rgba = vec![0u8; S * S * 4];
    for y in 0..S {
        for x in 0..S {
            let i = (y * S + x) * 4;
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            let corner = |cx: f32, cy: f32| ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt() > 6.0;
            let inside = (x >= 6 && x < 26 && y >= 1 && y < 31)
                || (y >= 6 && y < 26 && x >= 1 && x < 31)
                || (corner(7.0, 7.0) && x < 7 && y < 7)
                || (corner(25.0, 7.0) && x >= 25 && y < 7)
                || (corner(7.0, 25.0) && x < 7 && y >= 25)
                || (corner(25.0, 25.0) && x >= 25 && y >= 25);
            if !inside {
                continue;
            }
            let road_x = 26.0 - fy * 0.9;
            let on_road = (fx - road_x).abs() < 5.0;
            let dash = ((fy * 0.5).floor() as i32) % 2 == 0;
            if on_road {
                let centerline = (fx - road_x).abs() < 0.8;
                if centerline && dash {
                    rgba[i] = 245;
                    rgba[i + 1] = 245;
                    rgba[i + 2] = 244;
                } else {
                    rgba[i] = 217;
                    rgba[i + 1] = 119;
                    rgba[i + 2] = 6;
                }
            } else {
                rgba[i] = 30;
                rgba[i + 1] = 30;
                rgba[i + 2] = 30;
            }
            rgba[i + 3] = 255;
        }
    }
    (S as u32, S as u32, rgba)
}
