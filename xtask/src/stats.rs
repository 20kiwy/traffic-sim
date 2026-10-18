//! `cargo xtask stats`: lines of code per crate.

use crate::root;
use std::path::Path;

pub fn run(root: &Path) -> Result<(), String> {
    let mut rows: Vec<(String, usize, usize)> = Vec::new();
    for member in ["crates/gdmath", "crates/gdscript", "crates/roads", "crates/roadkit", "crates/egui-kit", "apps/roadkit", "apps/roadkit-cli", "xtask"] {
        let dir = root.join(member);
        if !dir.exists() {
            continue;
        }
        let (files, lines) = count(&dir);
        rows.push((member.to_string(), files, lines));
    }
    println!("{:<22} {:>6} {:>8}", "crate", "files", "lines");
    for (name, files, lines) in &rows {
        println!("{:<22} {:>6} {:>8}", name, files, lines);
    }
    let tf: usize = rows.iter().map(|r| r.1).sum();
    let tl: usize = rows.iter().map(|r| r.2).sum();
    println!("{:-<22} {:>6} {:>8}", "", "", "");
    println!("{:<22} {:>6} {:>8}", "total", tf, tl);
    Ok(())
}

fn count(dir: &Path) -> (usize, usize) {
    let mut files = 0;
    let mut lines = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap_or_else(|_| unreachable!()) {
            let Ok(e) = e else { continue };
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().map(|x| x == "rs").unwrap_or(false) {
                if let Ok(text) = std::fs::read_to_string(&p) {
                    files += 1;
                    lines += text.lines().count();
                }
            }
        }
    }
    (files, lines)
}
