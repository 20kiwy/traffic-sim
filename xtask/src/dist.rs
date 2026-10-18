//! `cargo xtask dist`: build the release binaries, render per-scenario screenshots
//! with the headless CLI, and package the versioned zip (source + screenshots).
//!
//! Ported from VectorCraft's xtask/bundle.rs, adapted to a source-zip deliverable:
//! the sandbox has no cross-compiler, so the artifact is the full workspace +
//! per-version screenshots, ready to `cargo run --release` anywhere.

use crate::{cargo, run, root, zip::ZipWriter};
use std::path::{Path, PathBuf};

/// The bundled scenarios rendered as screenshots by `dist`/`screenshots`.
/// (scenario, view, extra CLI args)
pub const SCENARIOS: &[(&str, &str, &str)] = &[
    ("city_grid", "plan", ""),
    ("roundabout_square", "plan", ""),
    ("brt_corridor", "plan", ""),
    ("highway_ramps", "plan", ""),
    ("texture_zoom", "plan", "--zoom 2.2"),
    ("traffic_junction", "plan", ""),
    ("dar_crossing", "plan", ""),
    ("lane_taper", "plan", ""),
    ("city_grid", "3d", "--ss 2"),
    ("roundabout_square", "3d", "--ss 2"),
    ("traffic_junction", "3d", "--ss 2 --sim 14 --focus 0,0,55"),
    ("highway_ramps", "3d", "--ss 2"),
    ("brt_corridor", "3d", "--ss 2"),
    ("texture_showcase", "3d", "--ss 2"),
    ("dar_crossing", "3d", "--ss 2 --sim 25 --focus -2,10,60"),
    ("lane_taper", "3d", "--ss 2"),
    ("prefab_4way", "3d", "--ss 2 --focus 0,0,30"),
];

pub fn dist_run(root: &Path, out: Option<String>) -> Result<(), String> {
    let version = crate::version::read(&std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?)?;
    let mut b = cargo();
    b.args(["build", "--release", "-p", "roadkit", "-p", "roadkit-cli"]);
    crate::run(b, "cargo build --release")?;
    let shots = screenshots(root, out.clone())?;
    let zip_path = package(root, &version, &shots, out.as_deref())?;
    println!("packaged {}", zip_path.display());
    Ok(())
}

/// Render every scenario's screenshot with the headless CLI. Returns the paths.
pub fn screenshots(root: &Path, out: Option<String>) -> Result<Vec<PathBuf>, String> {
    let out_dir = out.map(PathBuf::from).unwrap_or_else(|| root.join("dist/screenshots"));
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("mkdir {}: {e}", out_dir.display()))?;
    let version = crate::version::read(&std::fs::read_to_string(root.join("Cargo.toml")).map_err(|e| e.to_string())?)?;
    let dir = out_dir.join(format!("v{version}"));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let cli = target_dir(root).join("release/roadkit-cli");
    let mut made = Vec::new();
    for (scenario, view, extra) in SCENARIOS {
        let png = dir.join(format!("{scenario}_{view}.png"));
        let cmd = format!(
            "{} --script {} --view {} --out {} {} --size 1280x800",
            cli.display(),
            scenario,
            view,
            png.display(),
            extra
        );
        let mut c = std::process::Command::new(&cli);
        c.args(["--script", scenario, "--view", view, "--out"]);
        c.arg(&png);
        for a in extra.split_whitespace() {
            c.arg(a);
        }
        c.args(["--size", "1280x800"]);
        crate::run(c, &cmd)?;
        if !png.exists() {
            return Err(format!("cli did not produce {}", png.display()));
        }
        made.push(png);
    }
    Ok(made)
}

fn target_dir(root: &Path) -> PathBuf {
    std::env::var("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|_| root.join("target"))
}

/// Package the workspace source (minus target/dist/dotfiles) + screenshots +
/// release binaries into `roadkit-godot-<version>.zip`.
pub fn package(root: &Path, version: &str, shots: &[PathBuf], out: Option<&str>) -> Result<PathBuf, String> {
    let out_dir = out.map(PathBuf::from).unwrap_or_else(|| root.join("dist"));
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let path = out_dir.join(format!("roadkit-godot-{version}.zip"));

    // the rendered scenarios plus any UI captures already in the versioned dir
    let mut all_shots: Vec<PathBuf> = shots.to_vec();
    if let Some(dir) = shots.first().and_then(|s| s.parent()) {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.flatten() {
                let p = e.path();
                if p.extension().map(|e| e == "png").unwrap_or(false) && !all_shots.contains(&p) {
                    all_shots.push(p);
                }
            }
        }
    }

    let mut z = ZipWriter::new();
    let n_src = z
        .add_dir(root, "roadkit-godot", &|p: &Path| {
            crate::zip::skip_build(p)
                || p.extension().map(|e| e == "png" || e == "raw" || e == "glb").unwrap_or(false) && p.to_string_lossy().contains("tests")
        })
        .map_err(|e| e.to_string())?;
    let mut n_shots = 0;
    for s in &all_shots {
        let name = s.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default();
        z.add_file(s, &format!("roadkit-godot/screenshots/{name}")).map_err(|e| e.to_string())?;
        n_shots += 1;
    }
    let bytes = z.finish();
    std::fs::write(&path, &bytes).map_err(|e| format!("write {}: {e}", path.display()))?;
    println!("zip: {n_src} source files, {n_shots} screenshots, {} KB", bytes.len() / 1024);
    Ok(path)
}
