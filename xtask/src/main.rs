//! Workspace tooling: `cargo xtask <command>`.
//!
//! Ported from VectorCraft's xtask (pure Rust, std + serde_json). External tools
//! (`cargo`) are invoked through `std::process::Command`.
//!
//! commands:
//!   version [set X.Y.Z]  read / set the single source-of-truth workspace version
//!   dist [--out DIR]     build release binaries, render scenario screenshots,
//!                        package a versioned source+shots zip into DIR
//!   screenshots [--out DIR]  render the scenario screenshots only
//!   stats                lines of code per crate
//!   ci                   check + clippy -D warnings + selftest (stops at first failure)

mod dist;
mod stats;
mod version;
mod zip;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
usage: cargo xtask <command>

commands:
  version [set X.Y.Z]      the workspace version ([workspace.package] in the root Cargo.toml)
  dist [--out DIR]         build --release, render screenshots per scenario, package
                           roadkit-godot-<version>.zip (source + screenshots) into DIR
  screenshots [--out DIR]  render the bundled scenario screenshots only
  stats                    lines of code per crate
  ci                       cargo check, clippy -D warnings, app selftest
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let rest: Vec<&str> = args.iter().skip(1).map(String::as_str).collect();
    let result = match args.first().map(String::as_str) {
        Some("version") => version::run(&root(), &rest),
        Some("dist") => dist::dist_run(&root(), flag_value(&rest, "--out")),
        Some("screenshots") => dist::screenshots(&root(), flag_value(&rest, "--out")).map(|_| ()),
        Some("stats") => stats::run(&root()),
        Some("ci") => cmd_ci(),
        Some("-h" | "--help" | "help") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command `{other}`\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn flag_value(args: &[&str], flag: &str) -> Option<String> {
    args.iter().position(|a| *a == flag).and_then(|i| args.get(i + 1)).map(|s| s.to_string())
}

/// Workspace root (parent of the xtask crate).
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask has a parent dir").to_path_buf()
}

pub fn cargo() -> Command {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.current_dir(root());
    c
}

pub fn run(mut cmd: Command, what: &str) -> Result<(), String> {
    eprintln!("$ {what}");
    let status = cmd.status().map_err(|e| format!("{what}: failed to spawn: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what}: exited with {status}"))
    }
}

fn cmd_ci() -> Result<(), String> {
    type Step = (&'static str, Box<dyn Fn() -> Result<(), String>>);
    let steps: Vec<Step> = vec![
        (
            "check",
            Box::new(|| {
                let mut c = cargo();
                c.args(["check", "--workspace", "--all-targets"]);
                run(c, "cargo check --workspace --all-targets")
            }),
        ),
        (
            "clippy",
            Box::new(|| {
                let mut c = cargo();
                c.args(["clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]);
                run(c, "cargo clippy --workspace --all-targets -- -D warnings")
            }),
        ),
        (
            "selftest",
            Box::new(|| {
                let mut c = cargo();
                c.args(["run", "--release", "-p", "roadkit", "--", "--selftest"]);
                run(c, "cargo run --release -p roadkit -- --selftest")
            }),
        ),
    ];
    let mut done = Vec::new();
    for (name, f) in &steps {
        eprintln!("\n=== ci: {name} ===");
        if let Err(e) = f() {
            println!("\nCI summary:");
            for d in &done {
                println!("  ok    {d}");
            }
            println!("  FAIL  {name}: {e}");
            return Err(format!("ci failed at `{name}`"));
        }
        done.push(*name);
    }
    println!("\nCI summary: all {} steps passed ({})", done.len(), done.join(", "));
    Ok(())
}
