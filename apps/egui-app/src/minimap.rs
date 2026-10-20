//! 2D-on-top minimap — the plan (the SAME grid system the 3D ground shows)
//! rendered as a card overlay in the 3D viewport's bottom-left corner.
//! "the 3d grid is the 2d on top … so the 3d is seen on 2d": the live 3D
//! render stays underneath; the 2D map rides on top, auto-fitted to the
//! scene, with the camera target + heading marked. Click → full 2D editor.

use crate::state::{RoadkitApp, Tab};
use egui::{Color32, Pos2, Rect, Stroke};
use roads::mesh3d::{ASPHALT, MARKING_WHITE};

const CARD_W: f32 = 268.0;
const CARD_H: f32 = 178.0;

/// Metre ladder for the minimap grid: pick from 1-2-5×10ⁿ so a step lands
/// between ~24 and ~70 px at the fitted scale.
fn minimap_step(scale: f32) -> f64 {
    for cand in [1.0f64, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0, 2000.0, 5000.0] {
        if (cand as f32) * scale >= 24.0 {
            return cand;
        }
    }
    10_000.0
}

pub fn draw(app: &mut RoadkitApp, ui: &mut egui::Ui, viewport: Rect, _resp3d: &egui::Response) {
    let t = egui_kit::tokens(ui.ctx());
    let rect = Rect::from_min_size(
        Pos2::new(viewport.left() + 12.0, viewport.bottom() - CARD_H - 12.0),
        egui::vec2(CARD_W, CARD_H),
    );
    if !viewport.intersects(rect) {
        return;
    }

    let response = ui.allocate_rect(rect, egui::Sense::click());
    let painter = ui.painter();

    // card face
    painter.rect_filled(rect, 8.0, Color32::from_rgba_unmultiplied(22, 25, 31, 238));
    painter.rect_stroke(rect, 8.0, Stroke::new(1.0, t.border));
    if response.hovered() {
        painter.rect_stroke(rect, 8.0, Stroke::new(1.2, t.accent.gamma_multiply(0.8)));
    }

    // title + north arrow
    painter.text(
        Pos2::new(rect.left() + 9.0, rect.top() + 6.0),
        egui::Align2::LEFT_TOP,
        "2D PLAN",
        egui::FontId::proportional(10.0),
        t.text_dim,
    );
    painter.text(
        Pos2::new(rect.right() - 9.0, rect.top() + 6.0),
        egui::Align2::RIGHT_TOP,
        "N ↑",
        egui::FontId::proportional(10.0),
        t.text_dim,
    );

    // map area (below the title strip)
    let map = Rect::from_min_max(
        Pos2::new(rect.left() + 7.0, rect.top() + 22.0),
        Pos2::new(rect.right() - 7.0, rect.bottom() - 7.0),
    );

    // fit: bounds of the visible curves (plus a little pad)
    let scene = app.scene.borrow();
    let mut min_x = f64::INFINITY;
    let mut max_x = f64::NEG_INFINITY;
    let mut min_z = f64::INFINITY;
    let mut max_z = f64::NEG_INFINITY;
    for c in scene.curves.iter().filter(|c| c.visible) {
        for p in &c.points {
            min_x = min_x.min(p.x);
            max_x = max_x.max(p.x);
            min_z = min_z.min(p.z);
            max_z = max_z.max(p.z);
        }
    }
    if !min_x.is_finite() {
        min_x = -50.0;
        max_x = 50.0;
        min_z = -50.0;
        max_z = 50.0;
    }
    let span_x = (max_x - min_x).max(20.0);
    let span_z = (max_z - min_z).max(20.0);
    let k = ((map.width() / span_x as f32).min(map.height() / span_z as f32) * 0.86).max(0.001);
    let mid_x = (min_x + max_x) * 0.5;
    let mid_z = (min_z + max_z) * 0.5;
    let to_screen = |x: f64, z: f64| -> Pos2 {
        Pos2::new(
            map.center().x + ((x - mid_x) * k as f64) as f32,
            map.center().y + ((z - mid_z) * k as f64) as f32,
        )
    };

    // ---- the grid system (same metre ladder idea as the 3D ground) ----
    let step = minimap_step(k);
    let grid_col = Color32::from_rgba_unmultiplied(255, 255, 255, 13);
    let axis_col = Color32::from_rgba_unmultiplied(255, 255, 255, 24);
    let clip = painter.clip_rect().intersect(map);
    let p = ui.painter().with_clip_rect(clip);
    // vertical lines at x = n·step
    let x_start = ((mid_x - map.width() as f64 / k as f64 / 2.0) / step).floor() as i64;
    let x_end = ((mid_x + map.width() as f64 / k as f64 / 2.0) / step).ceil() as i64;
    for i in x_start..=x_end {
        let x = i as f64 * step;
        let a = to_screen(x, mid_z - span_z);
        let b = to_screen(x, mid_z + span_z);
        let col = if i == 0 { axis_col } else { grid_col };
        p.line_segment([a, b], Stroke::new(0.5, col));
    }
    let z_start = ((mid_z - map.height() as f64 / k as f64 / 2.0) / step).floor() as i64;
    let z_end = ((mid_z + map.height() as f64 / k as f64 / 2.0) / step).ceil() as i64;
    for i in z_start..=z_end {
        let z = i as f64 * step;
        let a = to_screen(mid_x - span_x, z);
        let b = to_screen(mid_x + span_x, z);
        let col = if i == 0 { axis_col } else { grid_col };
        p.line_segment([a, b], Stroke::new(0.5, col));
    }

    // ---- roads: dark casing → asphalt body → white edge peek (map style) ----
    for c in scene.curves.iter().filter(|c| c.visible) {
        let center = c.centerline(6.0);
        if center.len() < 2 {
            continue;
        }
        let pts: Vec<Pos2> = center.iter().map(|q| to_screen(q.x as f64, q.z as f64)).collect();
        let paved_px = (c.paved_width() * k as f64).max(1.4) as f32;
        p.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px + 1.6, Color32::from_rgb(26, 28, 32))));
        if paved_px >= 4.5 {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px, color32(MARKING_WHITE, 0.9))));
            p.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px - 1.4, color32(ASPHALT, 1.0))));
        } else {
            p.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px, color32(ASPHALT, 1.0))));
        }
        // bridge / flyover decks: amber so grade separation reads in 2D
        if c.has_elevated() {
            p.add(egui::Shape::line(
                pts.clone(),
                Stroke::new(1.2, Color32::from_rgba_unmultiplied(232, 176, 74, 210)),
            ));
        }
    }

    // ---- junction pads: quiet dots ----
    for j in &scene.junctions {
        let c = to_screen(j.x, j.z);
        if map.contains(c) {
            p.circle_filled(c, 2.6, color32(ASPHALT, 1.0));
            p.circle_stroke(c, 2.6, Stroke::new(0.8, Color32::from_rgba_unmultiplied(255, 255, 255, 90)));
        }
    }
    drop(scene);

    // ---- camera: target dot + heading wedge (where the 3D eye looks) ----
    let tgt = app.cam.target;
    let view = app.cam.view();
    // ground forward from the view matrix (third basis row, negated)
    let fwd = gdmath::vec3::Vec3::new(-view.m[8], 0.0, -view.m[10]).normalized();
    let tc = to_screen(tgt.x as f64, tgt.z as f64);
    let dir = egui::vec2(fwd.x, fwd.z); // world x,z → screen x,y (z flips down)
    let dl = dir.length().max(1e-4);
    let dir = dir / dl;
    let tip = tc + dir * 13.0;
    let perp = egui::vec2(-dir.y, dir.x);
    let wing = 4.6;
    let w1 = tc + dir * 4.0 + perp * wing;
    let w2 = tc + dir * 4.0 - perp * wing;
    p.add(egui::Shape::convex_polygon(
        vec![tip, w1, w2],
        t.accent.gamma_multiply(0.75),
        Stroke::NONE,
    ));
    p.circle_filled(tc, 2.6, t.accent);
    p.circle_stroke(tc, 4.4, Stroke::new(1.0, t.accent.gamma_multiply(0.5)));

    // step caption bottom-right of the map
    p.text(
        Pos2::new(map.right() - 2.0, map.bottom() - 2.0),
        egui::Align2::RIGHT_BOTTOM,
        format!("{step:.0} m grid"),
        egui::FontId::monospace(8.5),
        Color32::from_rgba_unmultiplied(255, 255, 255, 88),
    );

    if response.clicked() {
        app.tab = Tab::Plan;
    }
    response.on_hover_text("2D plan — the grid system on top of the 3D view. Click to open the full plan editor.");
}

fn color32(c: [u8; 3], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a * 255.0) as u8)
}
