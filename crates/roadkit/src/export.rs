//! RoadContainer → GLB export (GRG "Export RoadContainer").
//!
//! Builds the container's generated geometry the same way the plugin does
//! when it bakes a custom container: a flat ribbon swept along the curve's
//! `width_profile` (sampled at 4 m), lifted to the road surface y = 0.05,
//! with the curve's layer colour baked into vertex COLOR_0 and the same
//! single `vertexcolor` material the traffic pack uses. The byte packing
//! REUSES traffic.rs's `MeshBuilder` + `to_glb` (both pub — nothing there
//! is modified).

use roads::model::RoadCurve;
use std::path::Path;

use crate::traffic::{to_glb, MeshBuilder};

/// Road surface lift in meters (just above the ground plane, no z-fight).
const SURFACE_Y: f32 = 0.05;
/// Width-profile sample spacing (meters).
const SAMPLE_M: f64 = 4.0;

/// Export the given curves into one GLB at `out_path` (parent dirs are
/// created). Returns the triangle count written.
pub fn export_roads_glb(curves: &[RoadCurve], out_path: &Path) -> Result<usize, String> {
    let mut mb = MeshBuilder::new();
    for curve in curves {
        let profile = curve.width_profile(SAMPLE_M);
        if profile.len() < 2 {
            continue;
        }
        // vertex COLOR_0 from the curve's layer colour
        let col = [
            curve.color[0] as f32 / 255.0,
            curve.color[1] as f32 / 255.0,
            curve.color[2] as f32 / 255.0,
            1.0,
        ];
        for i in 0..profile.len() - 1 {
            let (p0, hw0) = profile[i];
            let (p1, hw1) = profile[i + 1];
            // segment tangent + its right-hand perpendicular (xz plane)
            let (tx, tz) = (p1.x - p0.x, p1.y - p0.y);
            let len = tx.hypot(tz);
            if len < 1e-6 {
                continue;
            }
            let (tx, tz) = (tx / len, tz / len);
            let (px, pz) = (-tz, tx); // right side of travel (model.rs right_dir)
            let left0 = [p0.x as f32 - (px * hw0) as f32, SURFACE_Y, p0.y as f32 - (pz * hw0) as f32];
            let right0 = [p0.x as f32 + (px * hw0) as f32, SURFACE_Y, p0.y as f32 + (pz * hw0) as f32];
            let right1 = [p1.x as f32 + (px * hw1) as f32, SURFACE_Y, p1.y as f32 + (pz * hw1) as f32];
            let left1 = [p1.x as f32 - (px * hw1) as f32, SURFACE_Y, p1.y as f32 - (pz * hw1) as f32];
            // (left0, right0, right1, left1) winding → +Y flat normals
            mb.quad(left0, right0, right1, left1, col);
        }
    }
    let tris = mb.tri_count();
    if tris == 0 {
        return Err("no geometry to export — the container has fewer than 2 points".into());
    }
    let bytes = to_glb(&mb);
    if let Some(parent) = out_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
    }
    std::fs::write(out_path, bytes)
        .map_err(|e| format!("cannot write {}: {e}", out_path.display()))?;
    Ok(tris)
}

/* ------------------------------------------------------------------ */
/* tests                                                               */
/* ------------------------------------------------------------------ */

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_writes_a_parseable_glb() {
        let mut c = RoadCurve::new("Export test");
        let mut a = roads::model::RoadPoint::new(-20.0, 0.0);
        a.lanes = 4;
        a.lane_width = 4.0;
        let mut b = roads::model::RoadPoint::new(20.0, 0.0);
        b.lanes = 2;
        b.lane_width = 3.5;
        c.add_point(a);
        c.add_point(b);

        let dir = std::env::temp_dir().join("roadkit_export_test");
        let path = dir.join("test_geo.glb");
        let tris = export_roads_glb(&[c], &path).expect("export ok");
        assert!(tris > 0, "ribbon must produce triangles");
        let bytes = std::fs::read(&path).expect("file written");
        let mesh = crate::glb::parse_glb(&bytes, "export").expect("valid GLB");
        assert_eq!(mesh.triangle_count(), tris);
        // 40 m of road at 4 m sampling → ~11 strips × 2 tris
        assert!(tris >= 16 && tris <= 28, "reasonable strip count, got {tris}");
        assert!(bytes.starts_with(b"glTF"), "GLB magic");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(dir);
    }

    #[test]
    fn empty_curve_refused() {
        let c = RoadCurve::new("empty");
        let dir = std::env::temp_dir().join("roadkit_export_test2");
        let path = dir.join("empty.glb");
        let err = export_roads_glb(&[c], &path).expect_err("must refuse");
        assert!(err.contains("no geometry"), "clear refusal: {err}");
        let _ = std::fs::remove_dir(dir);
    }
}
