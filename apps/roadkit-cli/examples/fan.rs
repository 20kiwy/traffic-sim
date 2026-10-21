//! Diagnostic: what scene geometry lives at ground level in the "fan"
//! artifact region south of the kijazi underpass?
use roadkit::scene::build_scene_enriched;

fn main() {
    let dsm = roadkit::build_dsm_local((0.0, 0.0), 700.0);
    let junctions = roads::junction::detect(&dsm.curves, &Default::default());
    let assets = {
        let mut c = std::env::current_dir().unwrap();
        loop {
            if c.join("assets/glb").exists() {
                break c.join("assets");
            }
            if !c.pop() {
                panic!("assets not found");
            }
        }
    };
    let library = roadkit::AssetLibrary::load_dir(&assets);
    let props = roadkit::dsm::dsm_props_capped(&dsm.curves, 720.0);
    let enrich = roadkit::dsm::dsm_buildings(&dsm.curves);
    let scene = build_scene_enriched(
        &dsm.curves,
        &junctions,
        &props,
        &[],
        &library,
        true,
        false,
        None,
        [0.7, 0.7, 0.7],
        &enrich,
    );
    let (cx, cz, _, _) = roadkit::scene_recenter(&dsm.curves);
    println!("scene center=({cx:.1},{cz:.1}) radius={:.1}", scene.radius_m);

    // the fan apex on screen sits just south of the underpass; probe a grid
    // in scene-local coords around (-cx, -cz) extended south (+z)
    for pz in [-10.0f32, -40.0, -80.0, -140.0, -220.0] {
        for px in [-60.0f32, -20.0, 20.0, 60.0, 100.0] {
            let wx = px - cx;
            let wz = pz - cz;
            let mut hits: Vec<String> = Vec::new();
            for (i, part) in scene.parts.iter().enumerate() {
                // bbox test in 2D
                let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
                let (mut z0, mut z1) = (f32::INFINITY, f32::NEG_INFINITY);
                let (mut y0, mut y1) = (f32::INFINITY, f32::NEG_INFINITY);
                for v in &part.positions {
                    x0 = x0.min(v.x);
                    x1 = x1.max(v.x);
                    z0 = z0.min(v.z);
                    z1 = z1.max(v.z);
                    y0 = y0.min(v.y);
                    y1 = y1.max(v.y);
                }
                if wx >= x0 && wx <= x1 && wz >= z0 && wz <= z1 && y0 < 4.0 {
                    hits.push(format!(
                        "part{i}[tex={},vcol={},c=({:.2},{:.2},{:.2}),y={y0:.2}..{y1:.2},n={}]",
                        part.tex,
                        !part.vcolors.is_empty(),
                        part.color[0],
                        part.color[1],
                        part.color[2],
                        part.positions.len()
                    ));
                }
            }
            // also ground parts
            for (i, part) in scene.ground.iter().enumerate() {
                let (mut x0, mut x1) = (f32::INFINITY, f32::NEG_INFINITY);
                let (mut z0, mut z1) = (f32::INFINITY, f32::NEG_INFINITY);
                for v in &part.positions {
                    x0 = x0.min(v.x);
                    x1 = x1.max(v.x);
                    z0 = z0.min(v.z);
                    z1 = z1.max(v.z);
                }
                if wx >= x0 && wx <= x1 && wz >= z0 && wz <= z1 {
                    hits.push(format!("ground{i}[tex={}]", part.tex));
                }
            }
            println!(
                "probe local({px:.0},{pz:.0}) -> {}",
                if hits.is_empty() { "-".into() } else { hits.join(" ") }
            );
        }
    }
}
