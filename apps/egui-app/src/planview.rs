//! 2D plan view — top-down editor over the script-built scene.
//! RoadPoint handles are drawn in LOGICAL PIXELS (crisp at every zoom).
//! Hidden layers (curve.visible / point.visible) are not drawn and not
//! pickable; locked points cannot be dragged (the layer-engine lock).

use crate::state::{DragKind, RoadkitApp, Tool};
use egui::{Color32, Pos2, Rect, Stroke};
use gdmath::vec2::Vec2;
use roadkit::props::asset_marker_color;
use roads::mesh3d::{ASPHALT, MARKING_WHITE, MARKING_YELLOW};

pub fn content(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(t.input))
        .show(ctx, |ui| {
            let rect = ui.available_rect_before_wrap();
            let response = ui.allocate_rect(rect, egui::Sense::click_and_drag());
            let painter = ui.painter().clone();

            // keyboard: Del removes the selected point
            if ctx.input(|i| i.key_pressed(egui::Key::Delete)) {
                app.delete_selected_point();
            }

            // gather curves snapshot
            let scene = app.scene.borrow();
            let curves = scene.curves.clone();
            let props = scene.props.clone();
            let junctions = scene.junctions.clone();
            let lanes = scene.lanes.clone();
            drop(scene);

            // ---------- grid ----------
            let step = pick_step(app.plan.scale);
            let step_px = ((step as f64) * app.plan.scale) as f32;
            let origin = app.plan_to_screen(Vec2::ZERO, &rect);
            let grid_col = Color32::from_rgba_unmultiplied(255, 255, 255, 7);
            let mut gx = origin.x % step_px;
            if gx < 0.0 {
                gx += step_px;
            }
            while gx < rect.right() {
                painter.line_segment([Pos2::new(gx, rect.top()), Pos2::new(gx, rect.bottom())], Stroke::new(0.5, grid_col));
                gx += step_px;
            }
            let mut gy = origin.y % step_px;
            if gy < 0.0 {
                gy += step_px;
            }
            while gy < rect.bottom() {
                painter.line_segment([Pos2::new(rect.left(), gy), Pos2::new(rect.right(), gy)], Stroke::new(0.5, grid_col));
                gy += step_px;
            }
            let axis_col = Color32::from_rgba_unmultiplied(255, 255, 255, 14);
            painter.line_segment(
                [Pos2::new(origin.x, rect.top()), Pos2::new(origin.x, rect.bottom())],
                Stroke::new(1.0, axis_col),
            );
            painter.line_segment(
                [Pos2::new(rect.left(), origin.y), Pos2::new(rect.right(), origin.y)],
                Stroke::new(1.0, axis_col),
            );

            // ---------- hovered grid cell: the cursor's surface highlight ----------
            // the cell under the pointer lights up (same metre ladder as the
            // lines above) — grid snapping becomes readable, in 2D exactly
            // like the 3D ground grid's cell highlight
            if let Some(hp) = response.hover_pos().filter(|_| !response.dragged()) {
                let wp = app.screen_to_plan(hp, &rect);
                let stepf = step as f64;
                let (cx, cz) = ((wp.x / stepf).floor() * stepf, (wp.y / stepf).floor() * stepf);
                let quad = [
                    app.plan_to_screen(Vec2::new(cx, cz), &rect),
                    app.plan_to_screen(Vec2::new(cx + stepf, cz), &rect),
                    app.plan_to_screen(Vec2::new(cx + stepf, cz + stepf), &rect),
                    app.plan_to_screen(Vec2::new(cx, cz + stepf), &rect),
                ];
                if quad.iter().all(|p| rect.contains(*p)) || rect.contains(quad[0]) {
                    let acc = t.accent;
                    painter.add(egui::Shape::convex_polygon(
                        quad.to_vec(),
                        Color32::from_rgba_unmultiplied(acc.r(), acc.g(), acc.b(), 30),
                        Stroke::new(1.2, acc.gamma_multiply(0.75)),
                    ));
                }
            }

            // ---------- curves, PASS 1: road bodies ----------
            // Draw order fixed (the old "messy vector" plan): bodies →
            // junction pads → markings. Junction polygons used to be painted
            // OVER the lane markings as opaque gray blobs.
            for (ci, c) in curves.iter().enumerate() {
                if !c.visible || c.points.is_empty() {
                    continue;
                }
                let center = c.centerline(4.0);
                if center.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = center
                    .iter()
                    .map(|p| app.plan_to_screen(Vec2::new(p.x as f64, p.z as f64), &rect))
                    .collect();
                let paved_px = (c.paved_width() * app.plan.scale).max(1.5) as f32;
                let selected = (app.sel.map(|(s, _)| s == ci).unwrap_or(false) || app.sel_container == Some(ci))
                    && app.sel_junction.is_none();

                let dirt = c.points.iter().any(|p| p.dirt);
                let surface = if dirt { [122, 96, 66] } else { ASPHALT };
                let casing = if dirt { [84, 64, 42] } else { [28, 30, 34] };

                if selected {
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px + 7.0, t.accent.gamma_multiply(0.5))));
                }
                // dark casing → white edge lines peeking at both sides →
                // asphalt body: a clean map-style road with round joins
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px + 2.4, color32(casing, 1.0))));
                if paved_px >= 6.0 {
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px, color32(MARKING_WHITE, 0.95))));
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px - 1.8, color32(surface, 1.0))));
                } else {
                    painter.add(egui::Shape::line(pts.clone(), Stroke::new(paved_px, color32(surface, 1.0))));
                }
            }

            // ---------- junction pads (under markings) ----------
            // egui::Shape::convex_polygon fans the point list, so concave
            // outlines (roundabouts, 6-leg junctions) rendered broken — fill
            // the CONVEX HULL in the road-surface colour instead.
            for (ji, j) in junctions.iter().enumerate() {
                let selected = app.sel_junction == Some(ji);
                let outline_px: Vec<Pos2> = j
                    .outline
                    .iter()
                    .map(|(x, z)| app.plan_to_screen(Vec2::new(*x, *z), &rect))
                    .collect();
                if outline_px.len() < 3 {
                    continue;
                }
                let hull = convex_hull(outline_px);
                let pad = if selected { color32([96, 118, 96], 1.0) } else { color32(ASPHALT, 1.0) };
                painter.add(egui::Shape::convex_polygon(
                    hull,
                    pad,
                    Stroke::new(1.0, Color32::from_rgb(0x28, 0x2a, 0x2e)),
                ));
                // roundabout island
                if let Some((ix, iz, ir)) = j.island {
                    let c = app.plan_to_screen(Vec2::new(ix, iz), &rect);
                    let r_px = (ir * app.plan.scale) as f32;
                    painter.circle_filled(c, r_px, color32([86, 138, 90], 1.0));
                    painter.circle_stroke(c, r_px, Stroke::new(1.0, Color32::from_rgb(0x2e, 0x4a, 0x30)));
                }
            }

            // ---------- curves, PASS 2: markings + labels ----------
            for (ci, c) in curves.iter().enumerate() {
                if !c.visible || c.points.is_empty() {
                    continue;
                }
                let center = c.centerline(4.0);
                if center.len() < 2 {
                    continue;
                }
                let pts: Vec<Pos2> = center
                    .iter()
                    .map(|p| app.plan_to_screen(Vec2::new(p.x as f64, p.z as f64), &rect))
                    .collect();
                let paved_px = (c.paved_width() * app.plan.scale).max(1.5) as f32;
                let selected = (app.sel.map(|(s, _)| s == ci).unwrap_or(false) || app.sel_container == Some(ci))
                    && app.sel_junction.is_none();

                // center markings (skip when the road is only a few px wide)
                if paved_px >= 3.0 {
                    if c.is_oneway() {
                        draw_dashed(&painter, &pts, color32(MARKING_WHITE, 0.9), 1.1, (8.0, 8.0));
                    } else if !c.points.iter().any(|p| p.dirt) {
                        painter.add(egui::Shape::line(pts.clone(), Stroke::new(1.1, color32(MARKING_YELLOW, 0.95))));
                    }
                }
                // median stripe when the cross-section has one
                let median_w = c.points.iter().map(|p| p.median).fold(0.0f64, f64::max);
                if median_w > 0.0 {
                    painter.add(egui::Shape::line(
                        pts.clone(),
                        Stroke::new((median_w * app.plan.scale).max(1.5) as f32, if median_w >= 3.0 { color32([190, 186, 178], 1.0) } else { color32([150, 146, 138], 1.0) }),
                    ));
                }
                // flyover / bridge deck styling (grade separation): amber
                // dashed halo + layer tag so decks read above ground roads
                if c.has_elevated() {
                    draw_dashed(
                        &painter,
                        &pts,
                        color32([232, 176, 74], 0.9),
                        (paved_px * 0.5).max(1.2),
                        (10.0, 7.0),
                    );
                }
                // label
                let first = pts[0];
                let tag = if c.has_elevated() { " · flyover L1" } else { "" };
                painter.text(
                    Pos2::new(first.x + 8.0, first.y - 8.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("{}{}", c.name, tag),
                    egui::FontId::proportional(11.0),
                    if selected { t.accent } else if c.has_elevated() { Color32::from_rgb(232, 176, 74) } else { t.text_dim },
                );
            }

            // ---------- AI lane overlays (GRG draw_lanes_editor) ----------
            // after markings, before point handles; only when really zoomed
            // in — at plan scale the arrowheads buried the whole map in
            // green noise (the old "messy vector" look)
            let mut lanes_drawn = false;
            if app.render.show_lanes && app.plan.scale >= 1.6 {
                for (ci, c) in curves.iter().enumerate() {
                    if !c.visible || !c.draw_lanes_editor || !c.generate_ai_lanes {
                        continue;
                    }
                    for lane in lanes.iter().filter(|l| l.container == ci) {
                        if lane.pts.len() < 2 {
                            continue;
                        }
                        let spts: Vec<Pos2> = lane
                            .pts
                            .iter()
                            .map(|(x, z)| app.plan_to_screen(Vec2::new(*x, *z), &rect))
                            .collect();
                        let (col, dashed) = if lane.transition {
                            (Color32::from_rgba_unmultiplied(150, 155, 165, 110), true)
                        } else if lane.side == 'F' {
                            (Color32::from_rgba_unmultiplied(0x3f, 0xcf, 0x8e, 200), false)
                        } else {
                            (Color32::from_rgba_unmultiplied(0xe8, 0xa1, 0x3d, 200), false)
                        };
                        if dashed {
                            draw_dashed(&painter, &spts, col, 1.2, (5.0, 6.0));
                        } else {
                            painter.add(egui::Shape::line(spts.clone(), Stroke::new(1.2, col)));
                        }
                        lanes_drawn = true;
                        // direction arrowheads every ~14 m along the lane
                        let mut travelled = 0.0f64;
                        let mut next_at = 7.0f64; // first arrow 7 m in
                        for w in lane.pts.windows(2) {
                            let (ax, az) = w[0];
                            let (bx, bz) = w[1];
                            let seg_m = ((bx - ax).hypot(bz - az)).max(1e-9);
                            while next_at <= travelled + seg_m {
                                let u = (next_at - travelled) / seg_m;
                                let px = ax + (bx - ax) * u;
                                let pz = az + (bz - az) * u;
                                let dx = (bx - ax) / seg_m;
                                let dz = (bz - az) / seg_m;
                                let sp = app.plan_to_screen(Vec2::new(px, pz), &rect);
                                // world dir -> screen dir (z flips to screen y)
                                let sd = egui::vec2(
                                    (dx * app.plan.scale) as f32,
                                    (dz * app.plan.scale) as f32,
                                );
                                let sl = sd.length();
                                if sl > 0.01 {
                                    let sd = sd / sl;
                                    let perp = egui::vec2(-sd.y, sd.x);
                                    let tip = sp + sd * 5.0;
                                    let l = sp - sd * 2.5 + perp * 2.8;
                                    let r = sp - sd * 2.5 - perp * 2.8;
                                    painter.add(egui::Shape::convex_polygon(
                                        vec![tip, l, r],
                                        col,
                                        Stroke::NONE,
                                    ));
                                }
                                next_at += 14.0;
                            }
                            travelled += seg_m;
                        }
                    }
                }
            }

            // ---------- junction labels + signal overlays ----------
            for (ji, j) in junctions.iter().enumerate() {
                let selected = app.sel_junction == Some(ji);
                let c = app.plan_to_screen(Vec2::new(j.x, j.z), &rect);
                if !rect.contains(c) {
                    continue;
                }
                painter.text(
                    Pos2::new(c.x, c.y - 10.0),
                    egui::Align2::CENTER_BOTTOM,
                    format!(
                        "{} · {} legs{}",
                        if j.kind == roads::JunctionKind::Roundabout { "roundabout" } else { "junction" },
                        j.legs.len(),
                        if app.signals.plan_at(j.x, j.z).is_some() { " · controlled" } else { "" },
                    ),
                    egui::FontId::proportional(9.5),
                    if selected { t.accent } else { t.text_dim },
                );
            }

            // ---------- signal overlays (registry + live sim state) ----------
            {
                let sim = app.render.sim.as_ref();
                for (ji, j) in junctions.iter().enumerate() {
                    let Some(plan) = app.signals.plan_at(j.x, j.z) else { continue };
                    let sp = app.plan_to_screen(Vec2::new(j.x, j.z), &rect);
                    if !rect.contains(sp) {
                        continue;
                    }
                    // one dot per phase group around the junction center
                    for gi in 0..plan.groups.len().max(1) {
                        let st = match sim {
                            Some(s) => plan.state_at(gi, s.t),
                            None => match plan.control {
                                roadkit::Control::Signal => roadkit::SignalState::Off,
                                roadkit::Control::Police => roadkit::SignalState::Amber,
                                _ => roadkit::SignalState::Off,
                            },
                        };
                        let rgb = st.rgb();
                        let col = Color32::from_rgb((rgb[0] * 255.0) as u8, (rgb[1] * 255.0) as u8, (rgb[2] * 255.0) as u8);
                        let ang = gi as f32 * std::f32::consts::TAU / plan.groups.len().max(1) as f32 - std::f32::consts::FRAC_PI_2;
                        let dot = Pos2::new(sp.x + ang.cos() * 11.0, sp.y + ang.sin() * 11.0);
                        painter.circle_filled(dot, 3.4, col);
                        painter.circle_stroke(dot, 3.4, Stroke::new(0.8, Color32::from_black_alpha(150)));
                    }
                }
                // L·TH·R passage labels at the selected junction
                if let (Some(s), Some(ji)) = (sim, app.sel_junction) {
                    if junctions.get(ji).is_some() {
                        for (x, z, _y, label) in s.approach_labels(&curves, ji) {
                            let sp = app.plan_to_screen(Vec2::new(x, z), &rect);
                            if !rect.contains(sp) {
                                continue;
                            }
                            painter.text(
                                Pos2::new(sp.x, sp.y - 6.0),
                                egui::Align2::CENTER_BOTTOM,
                                &label,
                                egui::FontId::monospace(9.5),
                                color32([255, 214, 120], 1.0),
                            );
                        }
                    }
                }
            }

            // ---------- props (small quiet dots — 541 of them used to be
            // loud confetti) ----------
            if app.plan.scale >= 0.1 {
                for p in &props {
                    let sp = app.plan_to_screen(Vec2::new(p.pos.x as f64, p.pos.z as f64), &rect);
                    if !rect.contains(sp) {
                        continue;
                    }
                    let c = asset_marker_color(&p.asset);
                    painter.circle_filled(sp, 2.2, color32(c, 0.85));
                }
            }

            // ---------- point handles (LOGICAL PIXELS — crisp) ----------
            for (ci, c) in curves.iter().enumerate() {
                if !c.visible {
                    continue;
                }
                for (pi, pt) in c.points.iter().enumerate() {
                    if !pt.visible {
                        continue;
                    }
                    let sp = app.plan_to_screen(Vec2::new(pt.x, pt.z), &rect);
                    if !rect.intersects(Rect::from_center_size(sp, egui::vec2(24.0, 24.0))) {
                        continue;
                    }
                    let is_sel = app.sel == Some((ci, pi));
                    let d = 3.0f32; // half-size: 6 px square, zoom-independent
                    let (fill, stroke) = if is_sel {
                        (t.accent, Stroke::new(1.5, t.text_strong))
                    } else if pt.locked {
                        (Color32::from_rgb(0x8a, 0x8f, 0x99), Stroke::new(1.0, Color32::from_black_alpha(160)))
                    } else {
                        (Color32::from_rgb(0xe8, 0xe8, 0xe8), Stroke::new(1.0, Color32::from_black_alpha(160)))
                    };
                    let quad = [
                        Pos2::new(sp.x - d, sp.y - d),
                        Pos2::new(sp.x + d, sp.y - d),
                        Pos2::new(sp.x + d, sp.y + d),
                        Pos2::new(sp.x - d, sp.y + d),
                    ];
                    painter.add(egui::Shape::convex_polygon(quad.to_vec(), fill, stroke));
                    if is_sel {
                        painter.circle_stroke(sp, 9.0, Stroke::new(1.2, t.accent));
                    }
                    if pt.locked {
                        painter.text(
                            Pos2::new(sp.x, sp.y - 8.0),
                            egui::Align2::CENTER_BOTTOM,
                            "🔒",
                            egui::FontId::proportional(8.0),
                            t.text_dim,
                        );
                    }
                }
            }

            // north arrow
            painter.text(
                Pos2::new(rect.right() - 14.0, rect.top() + 14.0),
                egui::Align2::RIGHT_TOP,
                "N ↑",
                egui::FontId::proportional(12.0),
                t.text_dim,
            );

            // scale bar
            let bar_m = pick_step(app.plan.scale);
            let bar_px = (bar_m as f64) * app.plan.scale;
            let bx = rect.left() + 14.0;
            let by = rect.bottom() - 14.0;
            painter.line_segment([Pos2::new(bx, by), Pos2::new(bx + bar_px as f32, by)], Stroke::new(2.0, t.text));
            painter.text(
                Pos2::new(bx + bar_px as f32 / 2.0, by - 4.0),
                egui::Align2::CENTER_BOTTOM,
                format!("{bar_m} m"),
                egui::FontId::proportional(10.0),
                t.text_dim,
            );

            // AI-lane legend (above the scale bar, only when lanes were drawn)
            if lanes_drawn {
                let ly = rect.bottom() - 32.0;
                let mut lx = bx;
                painter.text(
                    Pos2::new(lx, ly),
                    egui::Align2::LEFT_BOTTOM,
                    "▸ AI lanes:",
                    egui::FontId::proportional(10.0),
                    t.text_dim,
                );
                lx += 62.0;
                painter.line_segment(
                    [Pos2::new(lx, ly - 4.0), Pos2::new(lx + 10.0, ly - 4.0)],
                    Stroke::new(2.0, Color32::from_rgb(0x3f, 0xcf, 0x8e)),
                );
                painter.text(
                    Pos2::new(lx + 15.0, ly),
                    egui::Align2::LEFT_BOTTOM,
                    "forward",
                    egui::FontId::proportional(10.0),
                    Color32::from_rgba_unmultiplied(0x3f, 0xcf, 0x8e, 220),
                );
                lx += 70.0;
                painter.line_segment(
                    [Pos2::new(lx, ly - 4.0), Pos2::new(lx + 10.0, ly - 4.0)],
                    Stroke::new(2.0, Color32::from_rgb(0xe8, 0xa1, 0x3d)),
                );
                painter.text(
                    Pos2::new(lx + 15.0, ly),
                    egui::Align2::LEFT_BOTTOM,
                    "oncoming",
                    egui::FontId::proportional(10.0),
                    Color32::from_rgba_unmultiplied(0xe8, 0xa1, 0x3d, 220),
                );
            }

            // ---------- interaction ----------
            let pointer = ctx.input(|i| i.pointer.hover_pos()).filter(|p| rect.contains(*p));
            let world = pointer.map(|p| app.screen_to_plan(p, &rect));

            // cursor XYZ crosshair (Terrazon-style 2D guides): thin axis
            // lines through the pointer + a world-coordinate readout, and
            // the same value the 3D viewport gizmo reports
            if let Some((p, w)) = pointer.zip(world.clone()) {
                let guide = Color32::from_rgba_unmultiplied(214, 90, 82, 40);
                let guide_z = Color32::from_rgba_unmultiplied(96, 140, 230, 40);
                painter.line_segment(
                    [Pos2::new(rect.left(), p.y), Pos2::new(rect.right(), p.y)],
                    Stroke::new(1.0, guide_z),
                );
                painter.line_segment(
                    [Pos2::new(p.x, rect.top()), Pos2::new(p.x, rect.bottom())],
                    Stroke::new(1.0, guide),
                );
                painter.circle_stroke(p, 5.0, Stroke::new(1.0, t.text_dim));
                painter.text(
                    Pos2::new(p.x + 10.0, p.y - 8.0),
                    egui::Align2::LEFT_BOTTOM,
                    format!("x {:.1} · z {:.1}", w.x, w.y),
                    egui::FontId::monospace(10.0),
                    t.text,
                );
                app.cursor_world = Some([w.x, 0.0, w.y]);
            } else if app.tab == crate::state::Tab::Plan {
                app.cursor_world = None;
            }

            // snap indicator while dragging an endpoint near another endpoint
            if let Some(DragKind::Point { curve, point: _ }) = app.dragging {
                if let Some(w) = world {
                    let snap = app.scene.borrow().junction_settings.snap_m;
                    let enabled = app.scene.borrow().junction_settings.enabled;
                    if enabled {
                        let sc = app.scene.borrow();
                        for (i, c) in sc.curves.iter().enumerate() {
                            if i == curve || c.closed || c.points.is_empty() || !c.visible {
                                continue;
                            }
                            for p in [&c.points[0], c.points.last().unwrap()] {
                                if !p.visible || p.locked {
                                    continue;
                                }
                                let d = ((p.x - w.x).powi(2) + (p.z - w.y).powi(2)).sqrt();
                                if d <= snap {
                                    let sp = app.plan_to_screen(Vec2::new(p.x, p.z), &rect);
                                    let r_px = (snap * app.plan.scale).max(6.0) as f32;
                                    painter.circle_stroke(sp, r_px, Stroke::new(1.5, t.accent));
                                    painter.circle_filled(sp, 4.0, t.accent);
                                    painter.text(
                                        Pos2::new(sp.x, sp.y - r_px - 4.0),
                                        egui::Align2::CENTER_BOTTOM,
                                        "auto-join",
                                        egui::FontId::proportional(10.0),
                                        t.accent,
                                    );
                                }
                            }
                        }
                    }
                }
            }

            // drag behaviours
            if response.dragged() {
                let d = response.drag_delta();
                match app.dragging {
                    Some(DragKind::Pan { start_cx, start_cz, .. }) => {
                        app.plan.cx = start_cx - d.x as f64 / app.plan.scale;
                        app.plan.cz = start_cz - d.y as f64 / app.plan.scale;
                    }
                    Some(DragKind::Point { curve, point }) => {
                        if let Some(w) = world {
                            let mut sc = app.scene.borrow_mut();
                            if curve < sc.curves.len() && point < sc.curves[curve].points.len() {
                                let p = &mut sc.curves[curve].points[point];
                                if !p.locked && p.visible {
                                    p.x = w.x;
                                    p.z = w.y;
                                    sc.dirty = true;
                                }
                            }
                            drop(sc);
                            app.render.dirty = true;
                        }
                    }
                    None => {}
                }
            }
            if response.drag_stopped() {
                if let Some(DragKind::Point { curve, point }) = app.dragging {
                    app.weld_after_drag(curve, point);
                    let mut sc = app.scene.borrow_mut();
                    if curve < sc.curves.len() {
                        sc.curves[curve].auto_orient();
                        sc.dirty = true;
                    }
                    sc.log("[edit] point moved");
                }
                app.dragging = None;
            }

            // wheel zoom-about-pointer
            if response.hovered() {
                let scroll = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    if let Some(w) = world {
                        let factor = ((1.0 - scroll * 0.0012).clamp(0.5, 2.0)) as f64;
                        let new_scale = (app.plan.scale * factor).clamp(0.01, 80.0);
                        // keep the world point under the cursor fixed
                        app.plan.cx = w.x - (w.x - app.plan.cx) * (app.plan.scale / new_scale);
                        app.plan.cz = w.y - (w.y - app.plan.cz) * (app.plan.scale / new_scale);
                        app.plan.scale = new_scale;
                    }
                }
            }

            // clicks
            if response.clicked() {
                if let Some(w) = world {
                    match app.tool {
                        Tool::Pan => {}
                        Tool::AddPoint => app.add_point_at(w.x, w.y),
                        Tool::Stamp => app.stamp_at(w.x, w.y),
                        Tool::Select => {
                            // nearest VISIBLE point handle within 12 px, else junction center
                            let mut best: Option<(f64, usize, usize)> = None;
                            for (ci, c) in curves.iter().enumerate() {
                                if !c.visible {
                                    continue;
                                }
                                for (pi, pt) in c.points.iter().enumerate() {
                                    if !pt.visible || pt.locked {
                                        continue;
                                    }
                                    let sp = app.plan_to_screen(Vec2::new(pt.x, pt.z), &rect);
                                    let d = ((sp.x - pointer.unwrap().x) as f64).hypot((sp.y - pointer.unwrap().y) as f64);
                                    if d <= 12.0 && best.map(|(bd, _, _)| d < bd).unwrap_or(true) {
                                        best = Some((d, ci, pi));
                                    }
                                }
                            }
                            if let Some((_, ci, pi)) = best {
                                app.clear_selection();
                                app.sel = Some((ci, pi));
                                app.dock_tab = crate::state::DockTab::Properties;
                                let name = curves[ci].name.clone();
                                app.scene.borrow_mut().log(format!("[edit] selected point {pi} on '{name}'"));
                            } else {
                                // junction pick (within 14 px of the center)
                                let mut bj: Option<(f64, usize)> = None;
                                for (ji, j) in junctions.iter().enumerate() {
                                    let sp = app.plan_to_screen(Vec2::new(j.x, j.z), &rect);
                                    let d = ((sp.x - pointer.unwrap().x) as f64).hypot((sp.y - pointer.unwrap().y) as f64);
                                    if d <= 14.0 && bj.map(|(bd, _)| d < bd).unwrap_or(true) {
                                        bj = Some((d, ji));
                                    }
                                }
                                app.clear_selection();
                                app.sel_junction = bj.map(|(_, ji)| ji);
                                if let Some(ji) = app.sel_junction {
                                    app.dock_tab = crate::state::DockTab::Properties;
                                    let j = &junctions[ji];
                                    app.scene.borrow_mut().log(format!(
                                        "[edit] selected {} junction #{} — {} legs",
                                        j.kind.label(),
                                        ji + 1,
                                        j.legs.len()
                                    ));
                                }
                            }
                        }
                    }
                    // begin dragging the selected point if pressed on it
                    if app.tool == Tool::Select {
                        if let Some((ci, pi)) = app.sel {
                            if ci < curves.len() && pi < curves[ci].points.len() {
                                let pt = &curves[ci].points[pi];
                                if !pt.locked {
                                    let sp = app.plan_to_screen(Vec2::new(pt.x, pt.z), &rect);
                                    if let Some(p) = pointer {
                                        if ((sp.x - p.x) as f64).hypot((sp.y - p.y) as f64) <= 12.0 {
                                            app.dragging = Some(DragKind::Point { curve: ci, point: pi });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            // pan drag begin (Pan tool)
            if app.tool == Tool::Pan && response.drag_started() {
                app.dragging = Some(DragKind::Pan {
                    start_cx: app.plan.cx,
                    start_cz: app.plan.cz,
                });
            }
            if app.tool != Tool::Pan && app.dragging.is_none() && response.drag_started() {
                // middle-drag pans in any tool
                let mid = ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
                if mid {
                    app.dragging = Some(DragKind::Pan {
                        start_cx: app.plan.cx,
                        start_cz: app.plan.cz,
                    });
                }
            }
        });
}

fn pick_step(scale: f64) -> i64 {
    for s in [1i64, 2, 5, 10, 20, 50, 100, 200, 500, 1000, 2000, 5000] {
        if (s as f64) * scale >= 44.0 {
            return s;
        }
    }
    10_000
}

fn color32(c: [u8; 3], a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (a * 255.0) as u8)
}

fn draw_dashed(painter: &egui::Painter, pts: &[Pos2], color: Color32, width: f32, dash: (f32, f32)) {
    if pts.len() < 2 {
        return;
    }
    let mut drawing = true;
    let mut remaining = dash.0;
    let mut seg_start = pts[0];
    let mut cursor = seg_start;
    for &next in pts.iter().skip(1) {
        let a = cursor;
        let b = next;
        let len = (b - a).length();
        if len < 0.001 {
            continue;
        }
        let dir = ((b - a) / len).normalized();
        let mut t = 0.0f32;
        while t < len {
            let advance = remaining.min(len - t);
            let na = a + dir * t;
            let nb = a + dir * (t + advance);
            if drawing {
                painter.line_segment([if t == 0.0 { seg_start } else { na }, nb], Stroke::new(width, color));
            }
            t += advance;
            remaining -= advance;
            if remaining <= 0.001 {
                drawing = !drawing;
                remaining = if drawing { dash.0 } else { dash.1 };
                seg_start = nb;
            }
        }
        cursor = b;
        let _ = seg_start;
    }
}

/// Monotone-chain convex hull — egui's convex_polygon fans the raw point
/// list, so concave junction outlines (roundabouts, multi-leg pads) used to
/// fill as broken bowties. Filling the hull keeps the pad clean.
fn convex_hull(mut pts: Vec<Pos2>) -> Vec<Pos2> {
    pts.sort_by(|a, b| {
        a.x.partial_cmp(&b.x)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.y.partial_cmp(&b.y).unwrap_or(std::cmp::Ordering::Equal))
    });
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let cross = |o: &Pos2, a: &Pos2, b: &Pos2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<Pos2> = Vec::with_capacity(2 * pts.len() + 1);
    for p in &pts {
        while hull.len() >= 2 && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(*p);
    }
    let lower = hull.len() + 1;
    for p in pts.iter().rev() {
        while hull.len() >= lower && cross(&hull[hull.len() - 2], &hull[hull.len() - 1], p) <= 0.0 {
            hull.pop();
        }
        hull.push(*p);
    }
    hull.pop();
    hull
}
