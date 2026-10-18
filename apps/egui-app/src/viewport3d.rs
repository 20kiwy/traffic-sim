//! 3D viewport: software-rasterized scene blitted into an egui texture.
//! Orbit / pan / dolly like the GRG preview. This is where you SEE the
//! GLB props joined along the procedural roads. Hidden layers are filtered
//! before the scene build (the layer engine's eye).

use crate::state::{RoadkitApp, RenderState};
use egui::RichText;
use gdmath::vec3::Vec3;

/// Rebuild the render scene when the script scene changed.
pub fn ensure_scene(app: &mut RoadkitApp) {
    if app.render.dirty || app.render.scene3d.is_none() {
        let sc = app.scene.borrow();
        // layer engine: skip invisible containers + their props/junction legs
        let mut visible_curves: Vec<roads::model::RoadCurve> = Vec::new();
        let mut remap: Vec<Option<usize>> = Vec::with_capacity(sc.curves.len());
        for c in &sc.curves {
            if c.visible {
                remap.push(Some(visible_curves.len()));
                visible_curves.push(c.clone());
            } else {
                remap.push(None);
            }
        }
        // AI lanes: keep lanes of visible containers, remapped to the
        // compacted curve indices (lane.container indexes the FULL scene list)
        let vis_lanes: Vec<roads::lane::RoadLane> = sc
            .lanes
            .iter()
            .filter_map(|l| {
                remap.get(l.container).copied().flatten().map(|ci| {
                    let mut l = l.clone();
                    l.container = ci;
                    l
                })
            })
            .collect();
        let terrain = sc.terrain.clone();
        let scene = roadkit::scene::build_scene_full(
            &visible_curves,
            &sc.junctions,
            &sc.props,
            &vis_lanes,
            &app.assets,
            app.render.show_grid,
            app.render.show_lanes,
            Some(&terrain),
        );
        let radius = scene.radius_m;
        drop(sc);
        let first = app.render.scene3d.is_none();
        app.render.scene3d = Some(scene);
        app.render.dirty = false;
        if first {
            // hero 3/4 framing (same math spirit as roadkit-cli frame_cam):
            // fill the frame instead of floating far above the roads
            app.cam.target = Vec3::ZERO;
            app.cam.yaw = 0.55;
            app.cam.pitch = 0.55;
            app.cam.dist = (radius * 1.45).clamp(24.0, 20_000.0);
        }
    }
}

pub fn content(app: &mut RoadkitApp, ctx: &egui::Context) {
    ensure_scene(app);
    let t = egui_kit::tokens(ctx);

    // 3D-specific controls (global chrome lives in the control bar)
    egui::TopBottomPanel::top("v3d_controls")
        .frame(
            egui::Frame::none()
                .fill(t.panel)
                .stroke(egui::Stroke::new(1.0, t.border))
                .inner_margin(egui::Margin::symmetric(8.0, 2.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if egui_kit::btn(ui, "Top", false).on_hover_text("Top-down view").clicked() {
                    app.cam.pitch = 1.35;
                }
                if egui_kit::btn(ui, "45°", false).on_hover_text("Hero 45° view").clicked() {
                    app.cam.pitch = 0.52;
                    app.cam.yaw = 0.65;
                }
                ui.separator();
                ui.toggle_value(&mut app.render.show_grid, "grid").on_hover_text("Show the reference ground grid (environment is grid-only — no grass)");
                ui.toggle_value(&mut app.render.show_props, "props").on_hover_text("Show GLB props");
                ui.toggle_value(&mut app.render.show_lanes, "lanes").on_hover_text("Show the generated AI RoadLanes (GRG draw_lanes)");
                ui.toggle_value(&mut app.render.spin, "spin").on_hover_text("Auto-orbit the camera");
                if ui
                    .toggle_value(&mut app.render.sim_on, "traffic")
                    .on_hover_text("Simulate traffic — vehicle agents obey the junction signals")
                    .clicked()
                {
                    if app.render.sim_on && app.render.sim.is_none() {
                        let sc = app.scene.borrow();
                        app.render.sim = Some(roadkit::TrafficSim::new(
                            &sc.curves,
                            &sc.junctions,
                            roadkit::SimSettings { density: app.render.sim_density, ..Default::default() },
                        ));
                    }
                    app.render.dirty = true;
                }
                if app.render.sim_on {
                    ui.add(
                        egui::Slider::new(&mut app.render.sim_density, 4.0..=60.0)
                            .text("veh/km")
                            .integer(),
                    );
                }
                if egui_kit::btn(ui, "Rebuild", false).on_hover_text("Rebuild the scene mesh from the current script state").clicked() {
                    app.render.dirty = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new("LMB orbit · wheel zoom · RMB pan · R reset").size(10.5).color(t.text_dim));
                });
            });
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(t.input))
        .show(ctx, |ui| {
            let Some(scene) = app.render.scene3d.clone() else {
                ui.centered_and_justified(|ui| {
                    ui.label(RichText::new("Empty scene — run a .gd script (F5)").color(t.text_dim));
                });
                return;
            };

            // camera keys
            if ctx.input(|i| i.key_pressed(egui::Key::R)) {
                app.fit_scene();
            }

            let avail = ui.available_size();
            let w = avail.x.max(240.0).min(2560.0) as usize;
            let h = avail.y.max(180.0).min(1600.0) as usize;

            if app.render.spin {
                app.cam.yaw += 0.006;
            }

            let bg = [30, 34, 42, 255];
            // traffic sim: advance one frame step and render its vehicles
            let mut extra: Vec<roadkit::PropDraw> = Vec::new();
            if app.render.sim_on {
                let curves = {
                    let sc = app.scene.borrow();
                    sc.curves.clone()
                };
                if app.render.sim.is_none() {
                    let junctions = {
                        let sc = app.scene.borrow();
                        sc.junctions.clone()
                    };
                    app.render.sim = Some(roadkit::TrafficSim::new(
                        &curves,
                        &junctions,
                        roadkit::SimSettings { density: app.render.sim_density, ..Default::default() },
                    ));
                }
                if let Some(sim) = &mut app.render.sim {
                    sim.step(1.0 / 30.0, &curves);
                    extra = sim.draws(&curves, &app.assets);
                }
            } else if app.render.sim.is_some() {
                app.render.sim = None;
            }

            // Render cache: the CPU raster only runs when something actually
            // changed (camera, size, toggles, scene, sim). Idle frames reuse
            // the last texture — hover repaints cost ~0 ms.
            let sig = frame_sig(app, w, h, extra.len());
            let cache_hit = !app.render.dirty
                && sig == app.render.last_sig
                && app.render.tex.is_some()
                && extra.is_empty();
            let rgba;
            if cache_hit {
                rgba = Vec::new();
            } else {
                let t0 = std::time::Instant::now();
                let target = if app.render.show_props {
                    &scene
                } else {
                    // props hidden: render without prop draws (no scene clone)
                    &scene
                };
                rgba = roadkit::render_scene_props(target, &app.assets, &app.textures, &app.cam, w, h, bg, &extra, app.render.show_props);
                app.render.render_ms = t0.elapsed().as_secs_f32() * 1000.0;
                app.render.last_sig = sig;
                app.render.dirty = false;
            }

            if cache_hit {
                // reuse the uploaded texture — zero raster work this frame
            } else {
                let img = egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba);
                let tex = app.render.tex.get_or_insert_with(|| {
                    ctx.load_texture("viewport3d", img.clone(), egui::TextureOptions::LINEAR)
                });
                tex.set(img, egui::TextureOptions::LINEAR);
            }
            let Some(view_tex) = app.render.tex.as_ref() else {
                return;
            };

            let (rect, response) = ui.allocate_exact_size(egui::vec2(w as f32, h as f32), egui::Sense::click_and_drag());
            ui.painter().image(
                view_tex.id(),
                rect,
                egui::Rect::from_min_max(egui::Pos2::ZERO, egui::Pos2::new(1.0, 1.0)),
                egui::Color32::WHITE,
            );

            // ---- interaction ----
            let shift = ctx.input(|i| i.modifiers.shift);
            let secondary = ctx.input(|i| i.pointer.secondary_down());
            if response.dragged() {
                let d = response.drag_delta();
                if secondary || shift {
                    // pan in the camera plane
                    let view = app.cam.view();
                    let right = Vec3::new(view.m[0], view.m[1], view.m[2]);
                    let up = Vec3::new(view.m[4], view.m[5], view.m[6]);
                    let k = app.cam.dist * 0.0016;
                    app.cam.target = app.cam.target - right * (d.x * k) + up * (d.y * k);
                } else {
                    app.cam.yaw += d.x * 0.008;
                    app.cam.pitch = (app.cam.pitch + d.y * 0.006).clamp(-0.1, 1.45);
                }
            }
            if response.hovered() {
                let scroll = ctx.input(|i| i.raw_scroll_delta.y);
                if scroll != 0.0 {
                    app.cam.dist = (app.cam.dist * (1.0 + scroll * 0.0012)).clamp(4.0, 60_000.0);
                }
            }

            // ---- HUD ----
            let stats = scene.stats;
            let hud = format!(
                "{} curves · {} points · {} GLB props · {} tris · {:.0} m road · {:.1} ms/frame{}{}",
                stats.curves,
                stats.points,
                stats.props,
                stats.tris,
                stats.length_m,
                app.render.render_ms,
                if app.render.sim_on {
                    format!(
                        " · {} vehicles",
                        app.render.sim.as_ref().map(|s| s.vehicles.len()).unwrap_or(0)
                    )
                } else {
                    String::new()
                },
                if cache_hit { " · cached" } else { "" },
            );
            ui.painter().text(
                egui::Pos2::new(rect.left() + 10.0, rect.top() + 8.0),
                egui::Align2::LEFT_TOP,
                hud,
                egui::FontId::monospace(11.0),
                egui::Color32::from_rgba_unmultiplied(235, 235, 235, 220),
            );
            let hint = if app.render.show_props {
                "GLB props joined along procedural RoadPoint curves (GRG-style)"
            } else {
                "props hidden"
            };
            ui.painter().text(
                egui::Pos2::new(rect.left() + 10.0, rect.bottom() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                hint,
                egui::FontId::monospace(10.0),
                egui::Color32::from_rgba_unmultiplied(160, 160, 160, 200),
            );

            // ---- Godot-style axis gizmo (top-right, pure overlay) ----
            draw_axis_gizmo(app, ui, rect, &response);
        });
}

/// Hash everything that influences the rendered image (idle-frame cache key).
fn frame_sig(app: &RoadkitApp, w: usize, h: usize, extra: usize) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hsh = std::collections::hash_map::DefaultHasher::new();
    app.cam.yaw.to_bits().hash(&mut hsh);
    app.cam.pitch.to_bits().hash(&mut hsh);
    app.cam.dist.to_bits().hash(&mut hsh);
    app.cam.target.x.to_bits().hash(&mut hsh);
    app.cam.target.y.to_bits().hash(&mut hsh);
    app.cam.target.z.to_bits().hash(&mut hsh);
    {
        let sc = app.scene.borrow();
        sc.dirty.hash(&mut hsh);
        sc.lanes.len().hash(&mut hsh);
        sc.terrain.enabled.hash(&mut hsh);
    }
    app.render.show_grid.hash(&mut hsh);
    app.render.show_props.hash(&mut hsh);
    app.render.show_lanes.hash(&mut hsh);
    w.hash(&mut hsh);
    h.hash(&mut hsh);
    extra.hash(&mut hsh);
    hsh.finish()
}

#[allow(dead_code)]
fn silence(_r: &RenderState) {}

// ---------------------------------------------------------------------------
// Godot-style viewport axis gizmo
// ---------------------------------------------------------------------------

struct AxisEntry {
    axis: Vec3,
    letter: &'static str,
    color: egui::Color32,
    label: &'static str,
    /// camera snap: (yaw, pitch) — None keeps the current yaw (top/bottom)
    snap: Option<(f32, f32)>,
}

/// The six axis circles + the center "Perspective" reset dot, laid out like
/// the Godot viewport widget: +Y top, +X lower-right, +Z lower-left, dim
/// negatives opposite. Clicking a circle snaps the orbit camera onto that
/// axis (looking at the target); dragging anywhere on the widget orbits.
fn draw_axis_gizmo(app: &mut RoadkitApp, ui: &egui::Ui, rect: egui::Rect, response: &egui::Response) {
    const R: f32 = 40.0; // axis circle distance from the center
    const CR: f32 = 9.5; // circle radius (hit radius +2)

    // spec palette: X rgb(200,60,60), Y rgb(140,190,50), Z rgb(70,120,210);
    // negatives = same hue darkened ~50%
    let entries = [
        AxisEntry {
            axis: Vec3::new(1.0, 0.0, 0.0),
            letter: "X",
            color: egui::Color32::from_rgb(200, 60, 60),
            label: "Front (+X)",
            snap: Some((0.0, 0.02)),
        },
        AxisEntry {
            axis: Vec3::new(0.0, 1.0, 0.0),
            letter: "Y",
            color: egui::Color32::from_rgb(140, 190, 50),
            label: "Top (+Y)",
            snap: None, // pitch snap, keep yaw
        },
        AxisEntry {
            axis: Vec3::new(0.0, 0.0, 1.0),
            letter: "Z",
            color: egui::Color32::from_rgb(70, 120, 210),
            label: "Right (+Z)",
            snap: Some((std::f32::consts::FRAC_PI_2, 0.02)),
        },
        AxisEntry {
            axis: Vec3::new(-1.0, 0.0, 0.0),
            letter: "",
            color: egui::Color32::from_rgb(100, 30, 30),
            label: "Back (-X)",
            snap: Some((std::f32::consts::PI, 0.02)),
        },
        AxisEntry {
            axis: Vec3::new(0.0, -1.0, 0.0),
            letter: "",
            color: egui::Color32::from_rgb(70, 95, 25),
            label: "Bottom (-Y)",
            snap: None,
        },
        AxisEntry {
            axis: Vec3::new(0.0, 0.0, -1.0),
            letter: "",
            color: egui::Color32::from_rgb(35, 60, 105),
            label: "Left (-Z)",
            snap: Some((-std::f32::consts::FRAC_PI_2, 0.02)),
        },
    ];

    let painter = ui.painter();
    let center = egui::Pos2::new(rect.right() - 64.0, rect.top() + 64.0);

    // translucent rounded backdrop + faint outer ring (Godot-like)
    let bg = egui::Rect::from_center_size(center, egui::vec2(92.0, 92.0));
    painter.rect_filled(bg, 14.0, egui::Color32::from_rgba_unmultiplied(14, 17, 24, 140));
    painter.rect_stroke(
        bg,
        14.0,
        egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 14)),
    );
    painter.circle_stroke(center, 46.0, egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 16)));

    // screen direction of a world axis: (dot(a, right), -dot(a, up)) * R
    let view = app.cam.view();
    let cam_right = Vec3::new(view.m[0], view.m[1], view.m[2]);
    let cam_up = Vec3::new(view.m[4], view.m[5], view.m[6]);
    let axis_pos = |a: Vec3| -> egui::Pos2 {
        egui::Pos2::new(
            center.x + a.dot(cam_right) * R,
            center.y - a.dot(cam_up) * R,
        )
    };

    // thin spokes from the center to each POSITIVE circle
    for e in entries.iter().take(3) {
        painter.line_segment(
            [center, axis_pos(e.axis)],
            egui::Stroke::new(1.0, e.color.gamma_multiply(0.45)),
        );
    }

    // negative circles first (dim, no letters), then positives on top
    for e in entries.iter().skip(3) {
        painter.circle_filled(axis_pos(e.axis), CR, e.color);
        painter.circle_stroke(axis_pos(e.axis), CR, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(110)));
    }
    for e in entries.iter().take(3) {
        let p = axis_pos(e.axis);
        painter.circle_filled(p, CR, e.color);
        painter.circle_stroke(p, CR, egui::Stroke::new(1.0, egui::Color32::from_black_alpha(110)));
        if !e.letter.is_empty() {
            // pseudo-bold: dark shadow + white glyph
            painter.text(
                p + egui::vec2(0.7, 0.7),
                egui::Align2::CENTER_CENTER,
                e.letter,
                egui::FontId::proportional(11.0),
                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 150),
            );
            painter.text(
                p,
                egui::Align2::CENTER_CENTER,
                e.letter,
                egui::FontId::proportional(11.0),
                egui::Color32::WHITE,
            );
        }
    }

    // center dot: click = reset to the hero angle
    painter.circle_filled(center, 4.5, egui::Color32::from_rgba_unmultiplied(190, 196, 208, 200));
    painter.circle_stroke(center, 6.5, egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 40)));

    // hover tooltip (painted under the widget — deterministic, no popup lag)
    let pointer = response.hover_pos();
    let mut hovered_label: Option<&str> = None;
    if let Some(p) = pointer {
        if ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt() <= 7.0 {
            hovered_label = Some("Perspective");
        } else {
            for e in entries.iter() {
                let q = axis_pos(e.axis);
                if ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt() <= CR + 2.0 {
                    hovered_label = Some(e.label);
                    break;
                }
            }
        }
    }
    if let Some(label) = hovered_label {
        painter.text(
            egui::Pos2::new(center.x, center.y + 52.0),
            egui::Align2::CENTER_TOP,
            label,
            egui::FontId::proportional(10.5),
            egui::Color32::from_rgba_unmultiplied(235, 238, 245, 230),
        );
    }

    // click-to-snap (drag anywhere on the widget orbits via the rect drag)
    if response.clicked() {
        if let Some(p) = response.interact_pointer_pos() {
            let mut hit: Option<&AxisEntry> = None;
            if ((p.x - center.x).powi(2) + (p.y - center.y).powi(2)).sqrt() <= 7.0 {
                app.cam.yaw = 0.55;
                app.cam.pitch = 0.55;
            } else {
                for e in entries.iter() {
                    let q = axis_pos(e.axis);
                    if ((p.x - q.x).powi(2) + (p.y - q.y).powi(2)).sqrt() <= CR + 2.0 {
                        hit = Some(e);
                        break;
                    }
                }
                if let Some(e) = hit {
                    match e.snap {
                        Some((yaw, pitch)) => {
                            app.cam.yaw = yaw;
                            app.cam.pitch = pitch;
                        }
                        None => {
                            // +Y / -Y: top / bottom view (keep the yaw)
                            let top = e.axis.y > 0.0;
                            app.cam.pitch = if top { 1.5 } else { -1.5 };
                        }
                    }
                }
            }
        }
    }
}
