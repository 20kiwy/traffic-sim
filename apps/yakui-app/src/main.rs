//! yakui-app binary: `yakui-app --data kijazi|igesa [--shots N] [--size W] [--sim S]`

fn main() {
    let mut cfg = yakui_app::RunConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--data" => cfg.site = args.next().unwrap_or_else(|| "kijazi".into()),
            "--out" | "--out-prefix" => cfg.out_prefix = args.next().unwrap_or_else(|| "traffic_sim".into()),
            "--size" => {
                if let Some(v) = args.next() {
                    cfg.size = v.parse().unwrap_or(1280);
                }
            }
            "--shots" => {
                if let Some(v) = args.next() {
                    cfg.shots = v.parse().unwrap_or(3);
                }
            }
            "--sim" => {
                if let Some(v) = args.next() {
                    cfg.sim_seconds = v.parse().unwrap_or(14.0);
                }
            }
            "--no-hud" => cfg.hud = false,
            "--no-plan" => cfg.plan = false,
            other => {
                eprintln!("unknown arg {other}");
                std::process::exit(2);
            }
        }
    }
    match yakui_app::run_capture(&cfg) {
        Ok(files) => {
            println!("done: {} screenshots", files.len());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
