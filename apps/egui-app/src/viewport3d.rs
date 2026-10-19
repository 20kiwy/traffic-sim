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
                ui.toggle_value(&mut app.render.show_grid, "grid").on_hover_text("Show the TerraZone ground grid — clean screen-space reference lines hugging the engine terrain");
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

            // engine day environment: pale haze fog + zenith sky (yakui-app look)
            let env = roadkit::ENV_DAY;
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
                let t0 = web_time::Instant::now();
                let target = if app.render.show_props {
                    &scene
                } else {
                    // props hidden: render without prop draws (no scene clone)
                    &scene
                };
                rgba = roadkit::render_scene_props_env(target, &app.assets, &app.textures, &app.cam, w, h, env, &extra, app.render.show_props);
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

            // ---- TerraZone ground grid (screen-space reference lines) ----
            if app.render.show_grid {
                draw_ground_grid(app, ui, rect, &scene);
            }

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

// ---------------------------------------------------------------------------
// TerraZone ground grid (ported from TerraZone crates/ui-egui view3d.rs)
// ---------------------------------------------------------------------------

/// The ground grid's line spacing for the camera's distance: a 1-2-5×10ⁿ
/// metre ladder aiming at about four cells across the visible half-width,
/// so any dolly keeps a legible ladder. (TerraZone `grid_spacing`; the
/// viewport's fixed fov_y is 0.9 rad, as in `OrbitCam::proj`.)
fn grid_spacing(dist: f32) -> f32 {
    let raw = 2.0 * dist * 0.9f32.tan().max(0.01) / 4.0;
    let pow = 10.0f32.powf(raw.max(1e-3).log10().floor());
    let m = raw / pow;
    if m < 1.5 {
        pow
    } else if m < 3.5 {
        2.0 * pow
    } else if m < 7.5 {
        5.0 * pow
    } else {
        10.0 * pow
    }
}

/// The X/Z ground grid over the rendered engine terrain — TerraZone's
/// `draw_ground_grid` copied in: screen-space metre lines around the camera
/// target on the ground, the two centre lines stronger. Thin faint painter
/// strokes through the SAME view/projection as the CPU raster, so the lines
/// hug the heightfield, never alias, never z-fight and never blob at the
/// crossings.
/// The X/Z ground grid over the rendered engine terrain — TerraZone's
/// `draw_ground_grid` copied in: screen-space metre lines, thin and faint,
/// with the two centre lines stronger. Through the SAME view/projection as
/// the CPU raster so the lines hug the heightfield, never alias, never
/// z-fight and never blob at the crossings. Anchored at the scene centre
/// and clipped to the ground's own reach (grass fan = disc, terrain =
/// square) so lines never float off the ground onto the sky haze.
fn draw_ground_grid(app: &RoadkitApp, ui: &egui::Ui, rect: egui::Rect, scene: &roadkit::Scene) {
    let cam = &app.cam;
    let (w, h) = (rect.width(), rect.height());
    let view = cam.view();
    let proj = cam.proj(w / h);
    // world (scene-local) -> screen, exactly the raster's projection; also
    // returns the view depth (-z) for the distance-fog fade
    let xform = |p: [f32; 3]| -> Option<(egui::Pos2, f32)> {
        let v = view.transform_point(Vec3::new(p[0], p[1], p[2])).0;
        if v.z >= -0.05 {
            return None; // behind / on the near plane
        }
        let iw = -1.0 / v.z;
        let ndc_x = v.x * iw * proj.m[0];
        let ndc_y = v.y * iw * proj.m[5];
        Some((
            egui::Pos2::new(
                rect.min.x + (ndc_x * 0.5 + 0.5) * w,
                rect.min.y + (1.0 - (ndc_y * 0.5 + 0.5)) * h,
            ),
            -v.z,
        ))
    };
    // the raster's exact distance fog (draw_all): content fades to the haze
    // between these two camera depths — the lines must ride the SAME fade
    // or they float over fully-fogged ground
    let fog_start = cam.dist + scene.radius_m * 0.55;
    let fog_end = cam.dist + scene.radius_m * 3.4;

    let target = cam.target;
    let s = grid_spacing(cam.dist);
    let e = scene.ground_extent;
    // flat ground = the radial grass fan (disc); terrain = the square
    // heightfield — each mode clips its lines to its own shape
    let disc = scene.sampler.is_none();
    let lift = 0.03f32;
    let height = |x: f32, z: f32| -> f32 {
        scene
            .sampler
            .as_ref()
            .map(|g| g.height(x, z))
            .unwrap_or(0.0)
            + lift
    };
    // the lines must end BEFORE the ground melts into the sky haze — an
    // alpha-faded black line still contrasts against the near-sky ground,
    // so the strength is cut to zero while the ground is still clearly
    // green (the fan melts from 72%, the Chebyshev heightfield from 35%)
    let (rim_a, rim_b) = if disc { (0.55f32, 0.70f32) } else { (0.32f32, 0.55f32) };
    let fade = |x: f32, z: f32| -> f32 {
        let t = if disc {
            (x * x + z * z).sqrt() / e
        } else {
            (x.abs() / e).max(z.abs() / e)
        };
        let u = ((t - rim_a) / (rim_b - rim_a).max(1e-5)).clamp(0.0, 1.0);
        1.0 - u * u * (3.0 - 2.0 * u)
    };

    if std::env::var("RK_GRID_DEBUG").is_ok() {
        eprintln!("[grid] e={e:.0} disc={disc} s={s:.1} dist={:.1} target=({:.1},{:.1},{:.1}) radius={:.0} fog=({fog_start:.0},{fog_end:.0})",
            cam.dist, target.x, target.y, target.z, scene.radius_m);
    }
    let painter = ui.painter();
    // one ground-hugging polyline from (ax, az) to (bx, bz) inside the
    // ground shape; runs break at the near plane. `base` = the line's
    // alpha at the centre (melted out toward the rim)
    let ground_line = |ax: f32,
                       az: f32,
                       bx: f32,
                       bz: f32,
                       width: f32,
                       base: u8| {
        let span = (bx - ax).abs().max((bz - az).abs());
        let steps = ((span / s).ceil() as usize).clamp(1, 96);
        let mut prev: Option<egui::Pos2> = None;
        for k in 0..=steps {
            let t = k as f32 / steps as f32;
            let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
            if let Some((p, depth)) = xform([x, height(x, z), z]) {
                let fog = ((depth - fog_start) / (fog_end - fog_start)).clamp(0.0, 1.0);
                let a = base as f32 * fade(x, z) * (1.0 - fog);
                if a < 4.0 {
                    prev = None;
                    continue;
                }
                if let Some(q) = prev {
                    painter.line_segment(
                        [q, p],
                        egui::Stroke::new(width, egui::Color32::from_black_alpha(a as u8)),
                    );
                }
                prev = Some(p);
            } else {
                prev = None;
            }
        }
    };
    // only lines near the view (TerraZone's 28-cell window), anchored at
    // the scene centre like TerraZone anchors at the scene's position
    const VIEW_CELLS: f32 = 28.0;
    let line_lo = |t: f32| ((t - VIEW_CELLS * s) / s).floor() as i32;
    let line_hi = |t: f32| ((t + VIEW_CELLS * s) / s).ceil() as i32;

    for i in line_lo(target.x)..=line_hi(target.x) {
        let x = i as f32 * s;
        // clip this line's z range to the ground shape
        let (zmin, zmax) = if disc {
            let r2 = e * e - x * x;
            if r2 <= 0.0 {
                continue;
            }
            let r = r2.sqrt();
            (-r, r)
        } else {
            (-e, e)
        };
        let (za, zb) = (
            (zmin.max(line_lo(target.z) as f32 * s)).min(zmax),
            (zmax.min(line_hi(target.z) as f32 * s)).max(zmin),
        );
        if za >= zb {
            continue;
        }
        let (width, base) = if i == 0 { (1.4, 100u8) } else { (1.0, 44u8) };
        ground_line(x, za, x, zb, width, base);
    }
    for i in line_lo(target.z)..=line_hi(target.z) {
        let z = i as f32 * s;
        let (xmin, xmax) = if disc {
            let r2 = e * e - z * z;
            if r2 <= 0.0 {
                continue;
            }
            let r = r2.sqrt();
            (-r, r)
        } else {
            (-e, e)
        };
        let (xa, xb) = (
            (xmin.max(line_lo(target.x) as f32 * s)).min(xmax),
            (xmax.min(line_hi(target.x) as f32 * s)).max(xmin),
        );
        if xa >= xb {
            continue;
        }
        let (width, base) = if i == 0 { (1.4, 100u8) } else { (1.0, 44u8) };
        ground_line(xa, z, xb, z, width, base);
    }
}
