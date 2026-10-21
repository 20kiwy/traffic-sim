//! roadkit-cli — headless RoadKit: run a .gd scenario, render 2D plan + 3D view to PNG.
//!
//! USAGE
//!   roadkit-cli --script <name|path> --view plan|3d --out file.png [--zoom N]
//!   roadkit-cli --script <name|path> --view plan --out a.png --view 3d --out b.png

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return;
    }
    let code = cli_run(&args);
    std::process::exit(code);
}

fn print_help() {
    println!("roadkit-cli — headless RoadKit scenario renderer");
    println!();
    println!("USAGE");
    println!("  roadkit-cli --script <bundled|path.gd> [--view plan|3d --out f.png]... [--zoom N] [--size WxH] [--ss N] [--focus x,z,r]");
    println!("  roadkit-cli --opendrive <file.xodr> [--view 3d --out f.png] [--step 15] [--emit-script out.gd]");
    println!("  roadkit-cli --data kijazi|igesa [--shots N] [--size WxH] [--sim S] [--ss N] [--no-hud]   # OSM/Mapbox import");
    println!();
    println!("Bundled scripts are looked up in assets/scripts and assets/scenarios.");
}

fn arg_of(args: &[String], flag: &str) -> Option<String> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned()
}

fn cli_run(args: &[String]) -> i32 {
    // Dar es Salaam scenario: real OSM network + JICA TR2 overlay
    if args.iter().any(|a| a == "--dsm-appcam") {
        return match roadkit_cli::render_dsm_appcam(1280, 800) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    // single-site render: --dsm-site x,z,name (per-site camera env framing)
    if let Some(spec) = arg_of(args, "--dsm-site") {
        let parts: Vec<&str> = spec.splitn(3, ',').collect();
        if parts.len() == 3 {
            let sx: f64 = parts[0].trim().parse().unwrap_or(0.0);
            let sz: f64 = parts[1].trim().parse().unwrap_or(0.0);
            let name = parts[2].trim().to_string();
            let w: u32 = arg_of(args, "--size")
                .and_then(|s| s.split('x').next().and_then(|v| v.parse().ok()))
                .unwrap_or(1280);
            let h: u32 = arg_of(args, "--size")
                .and_then(|s| s.split('x').nth(1).and_then(|v| v.parse().ok()))
                .unwrap_or(800);
            let ss: usize = arg_of(args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(2).clamp(1, 4);
            let sim: f64 = arg_of(args, "--sim").and_then(|s| s.parse().ok()).unwrap_or(0.0);
            let density: f64 = arg_of(args, "--density").and_then(|s| s.parse().ok()).unwrap_or(14.0);
            return match roadkit_cli::render_dsm(&[(sx, sz, name.leak())], w, h, ss, sim, density) {
                Ok(files) => {
                    for f in &files {
                        println!("saved {f}");
                    }
                    0
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    1
                }
            };
        }
    }
    if args.iter().any(|a| a == "--dsm") {
        let w: u32 = arg_of(args, "--size")
            .and_then(|s| s.split('x').next().and_then(|v| v.parse().ok()))
            .unwrap_or(1280);
        let h: u32 = arg_of(args, "--size")
            .and_then(|s| s.split('x').nth(1).and_then(|v| v.parse().ok()))
            .unwrap_or(800);
        let ss: usize = arg_of(args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(2).clamp(1, 4);
        let sim: f64 = arg_of(args, "--sim").and_then(|s| s.parse().ok()).unwrap_or(20.0);
        let density: f64 = arg_of(args, "--density").and_then(|s| s.parse().ok()).unwrap_or(14.0);
        // Kijazi (0,0), Tazara (3261, 8647), Igesa (1680, -1592) + the
        // Fig 5.5.1 priority sites (Gerezani BRT flyover, Mwenge, Selanda)
        let sites = [
            (0.0, 0.0, "kijazi"),
            (3261.0, 8647.0, "tazara"),
            (1680.0, -1592.0, "igesa"),
            (4418.0, 6303.0, "gerezani"),
            (2907.0, -1912.0, "mwenge"),
            (8735.0, 8393.0, "selanda"),
        ];
        return match roadkit_cli::render_dsm(&sites, w, h, ss, sim, density) {
            Ok(files) => {
                for f in &files {
                    println!("saved {f}");
                }
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    // data-driven mode: import OSM/Mapbox raw data via tsdata + render
    if let Some(site) = arg_of(args, "--data") {
        let cfg = yakui_app::RunConfig {
            site,
            out_prefix: arg_of(args, "--out-prefix").unwrap_or_else(|| "traffic_sim".into()),
            size: arg_of(args, "--size")
                .and_then(|s| s.split('x').next().and_then(|w| w.parse().ok()))
                .unwrap_or(1280),
            ss: arg_of(args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(2).clamp(1, 4),
            shots: arg_of(args, "--shots").and_then(|s| s.parse().ok()).unwrap_or(3),
            sim_seconds: arg_of(args, "--sim").and_then(|s| s.parse().ok()).unwrap_or(14.0),
            hud: !args.iter().any(|a| a == "--no-hud"),
            plan: true,
        };
        return match yakui_app::run_capture(&cfg) {
            Ok(files) => {
                for f in &files {
                    println!("saved {f}");
                }
                0
            }
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    // ---- OpenDRIVE import: .xodr → native importer → .gd → world → views ----
    // roadkit-cli --opendrive <file.xodr> [--view 3d --out road.png] [--step 15]
    if let Some(xodr) = arg_of(args, "--opendrive") {
        let xml = std::fs::read_to_string(&xodr)
            .map_err(|e| format!("read {xodr}: {e}"))
            .unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(2);
            });
        let step: f64 = arg_of(args, "--step").and_then(|s| s.parse().ok()).unwrap_or(15.0);
        let map = roadkit::opendrive::OpenDriveMap::parse(&xml).unwrap_or_else(|e| {
            eprintln!("error: {e}");
            std::process::exit(2);
        });
        println!(
            "opendrive: rev {}.{:02} · {} roads · {} m total",
            map.rev_major,
            map.rev_minor,
            map.roads.len(),
            map.roads.iter().map(|r| r.length as usize).sum::<usize>()
        );
        let script = map.to_gd_script(step);
        if let Some(dst) = arg_of(args, "--emit-script") {
            std::fs::write(&dst, &script).map_err(|e| format!("write {dst}: {e}")).unwrap_or_else(|e| {
                eprintln!("error: {e}");
                std::process::exit(2);
            });
            println!("script dumped to {dst}");
        }

        let zoom: f32 = arg_of(args, "--zoom").and_then(|z| z.parse().ok()).unwrap_or(1.0);
        let ss: usize = arg_of(args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(1).clamp(1, 4);
        let sim_seconds: f64 = arg_of(args, "--sim").and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let density: f64 = arg_of(args, "--density").and_then(|s| s.parse().ok()).unwrap_or(14.0);
        let focus = arg_of(args, "--focus").and_then(|s| {
            let mut it = s.split(',');
            let x: f64 = it.next()?.trim().parse().ok()?;
            let z: f64 = it.next()?.trim().parse().ok()?;
            let r: f64 = it.next()?.trim().parse().ok()?;
            Some((x, z, r))
        });
        let size = arg_of(args, "--size").unwrap_or_else(|| "1280x800".into());
        let (w, h) = {
            let mut it = size.split('x');
            let w: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1280);
            let h: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(800);
            (w, h)
        };
        let mut views: Vec<(String, PathBuf)> = Vec::new();
        let mut it = args.iter();
        while let Some(a) = it.next() {
            if a == "--view" {
                let v = it.next().cloned().unwrap_or_else(|| "3d".into());
                let peek = it.next().cloned();
                let out = match peek {
                    Some(p) if !p.starts_with("--") => Some(p),
                    _ => arg_of(args, "--out"),
                };
                if let Some(o) = out {
                    views.push((v, PathBuf::from(o)));
                }
            }
        }
        if views.is_empty() {
            if let (Some(v), Some(o)) = (arg_of(args, "--view"), arg_of(args, "--out")) {
                views.push((v, PathBuf::from(o)));
            }
        }
        if views.is_empty() {
            views.push(("3d".into(), PathBuf::from("roadkit-opendrive.png")));
        }
        return match roadkit_cli::render_source_sim(&script, &views, zoom, w, h, ss, focus, sim_seconds, density) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: {e}");
                1
            }
        };
    }
    let Some(script) = arg_of(args, "--script") else {
        eprintln!("error: --script required");
        print_help();
        return 2;
    };
    let zoom: f32 = arg_of(args, "--zoom").and_then(|z| z.parse().ok()).unwrap_or(1.0);
    let ss: usize = arg_of(args, "--ss").and_then(|s| s.parse().ok()).unwrap_or(1).clamp(1, 4);
    let sim_seconds: f64 = arg_of(args, "--sim").and_then(|s| s.parse().ok()).unwrap_or(0.0);
    let density: f64 = arg_of(args, "--density").and_then(|s| s.parse().ok()).unwrap_or(14.0);
    let focus = arg_of(args, "--focus").and_then(|s| {
        let mut it = s.split(',');
        let x: f64 = it.next()?.trim().parse().ok()?;
        let z: f64 = it.next()?.trim().parse().ok()?;
        let r: f64 = it.next()?.trim().parse().ok()?;
        Some((x, z, r))
    });
    let size = arg_of(args, "--size").unwrap_or_else(|| "1280x800".into());
    let (w, h) = {
        let mut it = size.split('x');
        let w: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(1280);
        let h: u32 = it.next().and_then(|s| s.parse().ok()).unwrap_or(800);
        (w, h)
    };

    let mut views: Vec<(String, PathBuf)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--view" {
            let v = it.next().cloned().unwrap_or_else(|| "3d".into());
            // the out path may be positional right after the view, or given
            // later with --out (both accepted)
            let peek = it.next().cloned();
            let out = match peek {
                Some(p) if !p.starts_with("--") => Some(p),
                _ => arg_of(args, "--out"),
            };
            if let Some(o) = out {
                views.push((v, PathBuf::from(o)));
            }
        }
    }
    // fallback: a single pair via --view/--out only
    if views.is_empty() {
        if let (Some(v), Some(o)) = (arg_of(args, "--view"), arg_of(args, "--out")) {
            views.push((v, PathBuf::from(o)));
        }
    }
    if views.is_empty() {
        views.push(("3d".into(), PathBuf::from("roadkit-render.png")));
    }

    match roadkit_cli::render_named_sim(&script, &views, zoom, w, h, ss, focus, sim_seconds, density) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("error: {e}");
            1
        }
    }
}
