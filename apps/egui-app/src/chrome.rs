//! The Terrazon-style chrome: app bar (32) · control bar (34, tools live
//! here now) · central doc tabs. Status/hint bars live in statusbar.rs.
//! (The old 64 px left tool rail is removed — panels read cleaner.)
//!
//! LAYOUT BUG FIXED: the panel Ui is `top_down`; two bare `ui.with_layout`
//! calls stacked VERTICALLY — the right-aligned controls landed on an
//! invisible clipped second row (never seen) AND the overflow pushed every
//! following panel down, leaving unpainted black bands across the app.
//! Everything now lives inside ONE `ui.horizontal` row.

use crate::actions;
use crate::state::{DockTab, RoadkitApp, Tab, Tool};
use egui::RichText;
use egui_kit::IconName;

pub const APP_BAR_H: f32 = 32.0;
pub const CONTROL_H: f32 = 34.0;
pub const DOC_TAB_H: f32 = 35.0;

/* ------------------------------------------------------------------ */
/* App bar — brand, menus, look pickers                                */
/* ------------------------------------------------------------------ */

pub fn app_bar(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::TopBottomPanel::top("app_bar")
        .exact_height(APP_BAR_H)
        .frame(
            egui::Frame::none()
                .fill(t.app_bar)
                .inner_margin(egui::Margin::symmetric(10.0, 0.0)),
        )
        .show(ctx, |ui| {
            // ONE row: menus left, look pickers right (both vertically centered)
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(RichText::new("◆ RoadKit Godot").strong().size(12.0).color(t.text_strong));
                ui.separator();
                ui.spacing_mut().item_spacing.x = 2.0;

                ui.menu_button(RichText::new("Script").size(12.0), |ui| {
                    if ui.button("▶ Run script  (F5)").clicked() {
                        let _ = actions::perform(app, "run_script");
                        ui.close_menu();
                    }
                    if ui.button("Reset scene").clicked() {
                        let _ = actions::perform(app, "reset_scene");
                        ui.close_menu();
                    }
                    ui.separator();
                    // Dar es Salaam scenario — real OSM network + JICA TR2
                    // engineering overlay (Kijazi flyover, Tazara, Igesa)
                    if ui
                        .button("Load Dar es Salaam (JICA + OSM)")
                        .on_hover_text("Real DSM network: Sam Nujoma/Morogoro/Mandela/Nyerere from OSM + the Kijazi split flyover, Tazara 540 m flyover, BRT Ph-1 median, 1L·2TH·1R approaches, 120/130 s signal plans")
                        .clicked()
                    {
                        app.load_dsm();
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(RichText::new("bundled demos").size(10.0).color(t.text_dim));
                    for (i, (label, _, _)) in crate::scripts::all().into_iter().enumerate() {
                        let mut idx = app.script_idx;
                        if ui.selectable_label(idx == i, label).clicked() {
                            app.set_script(i);
                        }
                    }
                });
                ui.menu_button(RichText::new("View").size(12.0), |ui| {
                    for tab in [Tab::Scene3d, Tab::Plan] {
                        let label = match tab {
                            Tab::Scene3d => "3D Scene",
                            Tab::Plan => "2D Plan",
                        };
                        if ui.selectable_label(app.tab == tab, label).clicked() {
                            app.tab = tab;
                            ui.close_menu();
                        }
                    }
                    ui.separator();
                    if ui.button("Fit scene  (F)").clicked() {
                        let _ = actions::perform(app, "fit_scene");
                        ui.close_menu();
                    }
                    if ui.button("Reset 3D camera  (R)").clicked() {
                        let _ = actions::perform(app, "reset_camera");
                        ui.close_menu();
                    }
                    ui.separator();
                    ui.label(RichText::new("panels").size(10.0).color(t.text_dim));
                    for tab in DockTab::ALL {
                        let mut on = app.panels.is_open(tab);
                        if ui.checkbox(&mut on, tab.label()).changed() {
                            app.panels.set_open(tab, on);
                            if on {
                                app.dock_tab = tab;
                            }
                        }
                    }
                    if ui.checkbox(&mut app.dock_collapsed, "Collapse dock").changed() {
                        // keep as-is; the icon column shows instead
                    }
                });
                ui.menu_button(RichText::new("Props").size(12.0), |ui| {
                    if ui.button("▶ Join GLB props like Godot").clicked() {
                        let _ = actions::perform(app, "join_props");
                        ui.close_menu();
                    }
                    if ui.button("Clear all props").clicked() {
                        let _ = actions::perform(app, "clear_props");
                        ui.close_menu();
                    }
                });
                roads_menu(app, ui);
                ui.menu_button(RichText::new("Help").size(12.0), |ui| {
                    if ui.button("Road node structure").on_hover_text("What is the RoadManager, a RoadContainer, a RoadPoint, a RoadLane?").clicked() {
                        app.show_nodes_help = true;
                        ui.close_menu();
                    }
                    if ui.button(".gd scripting API").clicked() {
                        app.show_api = true;
                        ui.close_menu();
                    }
                    if ui.button("About RoadKit Godot").clicked() {
                        app.show_about = true;
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("List all UI actions (MCP)").clicked() {
                        let n = actions::all().len();
                        app.scene.borrow_mut().log(format!(
                            "[mcp] {n} registered UI actions — run `roadkit-godot --mcp` and call list_actions"
                        ));
                        ui.close_menu();
                    }
                });

                // right side: brightness + accent pickers
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for a in egui_kit::Accent::ALL {
                        let sel = app.accent == a;
                        let (r, resp) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::click());
                        let fill = if sel { accent_hex(a, true) } else { accent_hex(a, false) };
                        ui.painter().rect_filled(r, 7.0, fill);
                        if sel {
                            ui.painter().rect_stroke(r.shrink(1.5), 6.0, egui::Stroke::new(1.2, t.text_strong));
                        }
                        if resp.clicked() {
                            app.accent = a;
                            egui_kit::install(ctx, app.brightness, app.accent);
                        }
                        resp.on_hover_text(format!("{} accent", a.label()));
                    }
                    ui.separator();
                    for b in egui_kit::Brightness::ALL {
                        let sel = app.brightness == b;
                        if ui
                            .add(
                                egui::Button::new(RichText::new(b.label()).size(10.5).color(if sel { t.text_strong } else { t.text_dim }))
                                    .fill(if sel { t.tool_active } else { egui::Color32::TRANSPARENT })
                                    .min_size(egui::vec2(0.0, 20.0)),
                            )
                            .clicked()
                        {
                            app.brightness = b;
                            egui_kit::install(ctx, app.brightness, app.accent);
                        }
                    }
                });
            });
        });
}

/* ------------------------------------------------------------------ */
/* Roads menu — the GRG plugin's app-menu clone (plugin.gd "Roads")    */
/* ------------------------------------------------------------------ */

fn roads_item(app: &mut RoadkitApp, ui: &mut egui::Ui, label: &str, tip: &str, action: &str) {
    if ui.button(label).on_hover_text(tip).clicked() {
        let _ = actions::perform(app, action);
        ui.close_menu();
    }
}

fn roads_menu(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    ui.menu_button(RichText::new("Roads").size(12.0), |ui| {
        roads_item(app, ui, "Refresh roads", "Re-generate geometry and resolve warnings", "roads_refresh");
        roads_item(app, ui, "Select container", "Select this RoadPoint's parent RoadContainer", "roads_select_container");
        ui.separator();
        roads_item(app, ui, "RoadContainer", "Adds a RoadContainer child to RoadManager", "roads_add_container");
        roads_item(app, ui, "RoadPoint", "Adds a new RoadPoint", "roads_add_point");
        roads_item(app, ui, "RoadLane (AI path)", "Adds a RoadLane which can be used for AI paths", "roads_add_lane");
        roads_item(app, ui, "RoadLaneAgent (AI)", "Adds a RoadLaneAgent to follow RoadLane paths", "roads_add_agent");
        roads_item(app, ui, "RoadTerrain3DConnector", "Adds node to flatten Terrain3D surface based on roads", "roads_add_terrain_connector");
        ui.separator();
        roads_item(app, ui, "2x2 road", "Adds a segment of road with 2 lanes each way", "roads_2x2");
        ui.menu_button("RoadContainer presets →", |ui| {
            if ui.button("Change folder").on_hover_text("Pick a another folder containing tscn's with RoadContainer as the root").clicked() {
                app.scene.borrow_mut().log(
                    "[presets] folder picker stub — RoadKit ships the full GRG preset set bundled in assets/glb",
                );
                ui.close_menu();
            }
            if ui.button("Reset to default").on_hover_text("Reset to the default plugin intersection folder").clicked() {
                app.scene.borrow_mut().log(
                    "[presets] preset folder reset to the bundled set (10 custom containers)",
                );
                ui.close_menu();
            }
            if ui.button("Reload folder").on_hover_text("Refresh the current directory of tscn's").clicked() {
                let n = roads::prefab::PREFABS.len();
                app.scene
                    .borrow_mut()
                    .log(format!("[presets] reloaded {n} preset containers from the bundled folder"));
                ui.close_menu();
            }
            ui.separator();
            // the 10 GRG custom-container presets, alphabetically like the plugin
            for (id, tip) in [
                ("3way_1x1", "T-junction, single carriageway"),
                ("3way_2x2", "T-junction, dual carriageway"),
                ("4way_1x1", "4-way crossing, single carriageway"),
                ("4way_2x2", "4-way crossing, dual carriageway"),
                ("highway_offramp", "Highway off-ramp split"),
                ("highway_onramp", "Highway on-ramp merge"),
                ("roundabout_1x1", "Roundabout, single carriageway"),
                ("roundabout_2x2", "Roundabout, dual carriageway"),
                ("splitter_1x1", "Lane splitter, single carriageway"),
                ("splitter_2x2", "Lane splitter, dual carriageway"),
            ] {
                if ui
                    .button(id)
                    .on_hover_text(format!("Spawn the bundled '{id}' preset — {tip}"))
                    .clicked()
                {
                    let _ = actions::perform(app, &format!("roads_preset_{id}"));
                    ui.close_menu();
                }
            }
        });
        ui.separator();
        roads_item(app, ui, "Export RoadContainer", "Export current RoadContainer and children to a gLTF/GLB file", "roads_export");
        roads_item(app, ui, "Make RoadLanes Editable", "Expose RoadLanes nodes in editor for manual editing", "roads_make_lanes_editable");
        ui.separator();
        roads_item(app, ui, "Report issue", "Report an issue with the plugin on GitHub", "roads_report_issue");
        roads_item(app, ui, "Share feedback", "Share feedback about the plugin", "roads_share_feedback");
    });
}

fn accent_hex(a: egui_kit::Accent, strong: bool) -> egui::Color32 {
    use egui_kit::Accent::*;
    let hex = match (a, strong) {
        (Coral, false) => 0xf06a52,
        (Coral, true) => 0xe8573f,
        (Blue, false) => 0x5b96ff,
        (Blue, true) => 0x2f74f0,
        (Violet, false) => 0xa98bff,
        (Violet, true) => 0x7c55f0,
        (Green, false) => 0x3fcf8e,
        (Green, true) => 0x1f9d63,
        (Amber, false) => 0xf5b544,
        (Amber, true) => 0xc47f0e,
        (Pink, false) => 0xf36aa3,
        (Pink, true) => 0xdb3f7c,
        _ => 0xf06a52,
    };
    egui::Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/* ------------------------------------------------------------------ */
/* Control bar — the Illustrator options strip under the app bar       */
/* ------------------------------------------------------------------ */

pub fn control_bar(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::TopBottomPanel::top("control_bar")
        .exact_height(CONTROL_H)
        .frame(
            egui::Frame::none()
                .fill(t.panel)
                .stroke(egui::Stroke::new(1.0, t.border))
                .inner_margin(egui::Margin::symmetric(10.0, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                // tools — the old left rail's four tools, now inline like the
                // Terrazon top bar (icon + hotkey chip, fixed 48 px each —
                // icon_button_caption would eat the whole row width here)
                tool_chip(ui, app, Tool::Select, IconName::Cursor);
                tool_chip(ui, app, Tool::AddPoint, IconName::Point);
                tool_chip(ui, app, Tool::Stamp, IconName::Stamp);
                tool_chip(ui, app, Tool::Pan, IconName::Hand);
                ui.separator();
                ui.label(RichText::new(app.tool.label()).strong().size(12.0).color(t.text_strong));
                ui.separator();

                // junction engine toggles
                let mut js = app.scene.borrow().junction_settings.clone();
                let mut changed = false;
                if ui
                    .checkbox(&mut js.enabled, "auto-join")
                    .on_hover_text("Weld nearby endpoints + detect crossings automatically")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut js.auto_roundabout, "roundabouts")
                    .on_hover_text("3+ road meetings become roundabouts with islands")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut js.use_prefabs, "prefabs")
                    .on_hover_text("WFC-match hand-modeled GLB intersection tiles (GRG custom containers)")
                    .changed()
                {
                    changed = true;
                }
                ui.label(RichText::new("snap").size(11.0).color(t.text_dim));
                if ui
                    .add(egui::DragValue::new(&mut js.snap_m).speed(0.5).suffix(" m").range(1.0..=50.0))
                    .changed()
                {
                    changed = true;
                }
                if changed {
                    app.scene.borrow_mut().junction_settings = js;
                    app.scene.borrow_mut().recompute_junctions();
                    app.render.dirty = true;
                }

                let junctions = app.scene.borrow().junctions.len();
                ui.separator();
                ui.label(
                    RichText::new(format!(" intersections {junctions}"))
                        .size(11.0)
                        .color(t.text_dim),
                );

                // right side: doc tab switch, camera buttons, Run
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    // primary Run button
                    if ui
                        .add(
                            egui::Button::new(RichText::new("▶ Run").strong().size(12.0).color(t.text_strong))
                                .fill(t.accent_strong)
                                .min_size(egui::vec2(64.0, 23.0)),
                        )
                        .on_hover_text("Rebuild the loaded scene — or run the .gd script when no city is loaded (F5)")
                        .clicked()
                    {
                        let _ = actions::perform(app, "rebuild_or_run");
                    }
                    ui.separator();
                    if egui_kit::btn(ui, "Fit", false).on_hover_text("Fit the scene in view (F)").clicked() {
                        let _ = actions::perform(app, "fit_scene");
                    }
                    if egui_kit::btn(ui, "Cam", false).on_hover_text("Reset the 3D orbit camera (R)").clicked() {
                        let _ = actions::perform(app, "reset_camera");
                    }
                    ui.separator();
                    // doc tab switch (3D/2D)
                    for (tab, label) in [(Tab::Scene3d, "3D"), (Tab::Plan, "2D")] {
                        let sel = app.tab == tab;
                        if ui
                            .add(
                                egui::Button::new(RichText::new(label).size(11.5).color(if sel { t.text_strong } else { t.text_dim }))
                                    .fill(if sel { t.tool_active } else { egui::Color32::TRANSPARENT })
                                    .min_size(egui::vec2(30.0, 22.0)),
                            )
                            .on_hover_text(if tab == Tab::Plan { "2D plan editor" } else { "3D scene render" })
                            .clicked()
                        {
                            app.tab = tab;
                        }
                    }
                });
            });
        });
}

/// One compact tool toggle for the control bar: icon + hotkey letter in a
/// fixed 48×26 chip (the control bar is only 34 px tall — the rail's
/// icon_button_caption is 40 px tall AND eats available_width, so the bar
/// needs its own widget).
fn tool_chip(ui: &mut egui::Ui, app: &mut RoadkitApp, tool: Tool, icon: IconName) {
    let t = egui_kit::tokens(ui.ctx());
    let sel = app.tool == tool;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(48.0, 26.0), egui::Sense::click());
    if sel {
        ui.painter().rect_filled(rect, 5.0, t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 5.0, t.hover);
    }
    egui_kit::paint(
        ui,
        icon,
        egui::Rect::from_center_size(egui::pos2(rect.left() + 14.0, rect.center().y), egui::vec2(14.0, 14.0)),
        if sel { t.text_strong } else { t.icon },
    );
    ui.painter().text(
        egui::pos2(rect.right() - 10.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        tool.hotkey(),
        egui::FontId::monospace(9.5),
        if sel { t.text_strong } else { t.text_dim },
    );
    if resp.clicked() {
        app.tool = tool;
    }
    resp.on_hover_text(tool.label());
}

/* ------------------------------------------------------------------ */
/* Central — doc tab strip + view dispatch                             */
/* ------------------------------------------------------------------ */

pub fn central(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::TopBottomPanel::top("doc_tabs")
        .exact_height(DOC_TAB_H)
        .frame(egui::Frame::none().fill(t.pasteboard))
        .show(ctx, |ui| doc_tabs(app, ui));
    match app.tab {
        Tab::Scene3d => crate::viewport3d::content(app, ctx),
        Tab::Plan => crate::planview::content(app, ctx),
    }
}

fn doc_tabs(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = egui_kit::tokens(ui.ctx());
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for (tab, label) in [(Tab::Plan, "2D Plan — snapping & junctions"), (Tab::Scene3d, "3D Scene — procedural roads · GLB props")] {
            let sel = app.tab == tab;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(210.0, DOC_TAB_H - 1.0), egui::Sense::click());
            if sel {
                ui.painter().rect_filled(rect, 0.0, t.panel);
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, t.hover.gamma_multiply(0.4));
            }
            ui.painter().line_segment(
                [rect.right_top(), rect.right_bottom()],
                egui::Stroke::new(1.5, if sel { t.border } else { egui::Color32::TRANSPARENT }),
            );
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::proportional(if sel { 12.5 } else { 12.0 }),
                if sel { t.text_strong } else { t.text_dim },
            );
            if resp.clicked() {
                app.tab = tab;
            }
        }
    });
}
