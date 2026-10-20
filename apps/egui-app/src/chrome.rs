//! The Terrazon-style chrome: app bar (32) · control bar (34, tools live
//! here now) · central doc tabs. Status/hint bars live in statusbar.rs.
//!
//! R13 de-clutter rework: FOUR menus only (Scene / Roads / Bridges / Help),
//! no look pickers (one theme, one accent), no Props menu (props dress the
//! roads through scenes + the Assets panel on the icon column). The app is
//! ROADS + BRIDGES only — buildings stay parked in the code as a backup.

use crate::actions;
use crate::state::{RoadkitApp, Tab, Tool};
use egui::RichText;
use egui_kit::IconName;
use roads::model::BridgeStyle;

pub const APP_BAR_H: f32 = 32.0;
pub const CONTROL_H: f32 = 34.0;
pub const DOC_TAB_H: f32 = 35.0;

/* ------------------------------------------------------------------ */
/* App bar — brand + the four menus                                    */
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
            ui.horizontal_centered(|ui| {
                ui.spacing_mut().item_spacing.x = 10.0;
                ui.label(RichText::new("◆ RoadKit").strong().size(12.0).color(t.text_strong));
                ui.separator();
                ui.spacing_mut().item_spacing.x = 2.0;

                ui.menu_button(RichText::new("Scene").size(12.0), |ui| {
                    if ui.button("▶ Run  (F5)").on_hover_text("Rebuild the loaded scene — or run the .gd script when no city is loaded").clicked() {
                        let _ = actions::perform(app, "rebuild_or_run");
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
                roads_menu(app, ui);
                bridges_menu(app, ui);
                ui.menu_button(RichText::new("Help").size(12.0), |ui| {
                    if ui.button("Road node structure").on_hover_text("What is the RoadManager, a RoadContainer, a RoadPoint, a RoadLane?").clicked() {
                        app.show_nodes_help = true;
                        ui.close_menu();
                    }
                    if ui.button(".gd scripting API").clicked() {
                        app.show_api = true;
                        ui.close_menu();
                    }
                    if ui.button("About RoadKit").clicked() {
                        app.show_about = true;
                        ui.close_menu();
                    }
                });

                // right side: the 3D / 2D doc switch (no look pickers —
                // one theme, one accent)
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    for (tab, label) in [(Tab::Plan, "2D"), (Tab::Scene3d, "3D")] {
                        let sel = app.tab == tab;
                        if ui
                            .add(
                                egui::Button::new(RichText::new(label).size(11.5).color(if sel { t.text_strong } else { t.text_dim }))
                                    .fill(if sel { t.tool_active } else { egui::Color32::TRANSPARENT })
                                    .min_size(egui::vec2(30.0, 22.0)),
                            )
                            .on_hover_text(if tab == Tab::Plan { "2D plan editor" } else { "3D scene render — the 2D plan rides on top as a minimap" })
                            .clicked()
                        {
                            app.tab = tab;
                        }
                    }
                });
            });
        });
}

/* ------------------------------------------------------------------ */
/* Roads menu — the roads-only core of the GRG plugin's app menu       */
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
        roads_item(app, ui, "Add RoadPoint", "Adds a new RoadPoint", "roads_add_point");
        roads_item(app, ui, "2x2 road", "Adds a segment of road with 2 lanes each way", "roads_2x2");
        ui.menu_button("Road presets →", |ui| {
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
        roads_item(app, ui, "Fit scene  (F)", "Fit the whole scene in view", "fit_scene");
        roads_item(app, ui, "Reset camera  (R)", "Reset the 3D orbit camera", "reset_camera");
    });
}

/* ------------------------------------------------------------------ */
/* Bridges menu — the RoadArchitect bridge kit, one place: deck style  */
/* (girder / box girder / arch / suspension) + tunnel + edge safety    */
/* (guardrails / cable barrier), applied to the selected RoadContainer */
/* ------------------------------------------------------------------ */

fn bridges_menu(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = egui_kit::tokens(ui.ctx());
    ui.menu_button(RichText::new("Bridges").size(12.0), |ui| {
        let sel = app.sel_container.or(app.sel.map(|(c, _)| c));
        let (style, tunnel, guardrail, cable) = match sel {
            Some(ci) => {
                let sc = app.scene.borrow();
                sc.curves.get(ci).map(|c| (c.bridge_style, c.tunnel, c.guardrail, c.cable_barrier)).unwrap_or((BridgeStyle::Girder, false, false, false))
            }
            None => (BridgeStyle::Girder, false, false, false),
        };
        ui.label(
            RichText::new(match sel {
                Some(ci) => format!("selected: {}", app.scene.borrow().curves.get(ci).map(|c| c.name.clone()).unwrap_or_default()),
                None => "no road selected — pick one in Layers or the 2D plan".to_string(),
            })
            .size(10.0)
            .color(t.text_dim),
        );
        ui.label(RichText::new("deck style").size(10.0).color(t.text_dim));
        for (label, action, st) in [
            ("Girder deck", "bridge_style_girder", BridgeStyle::Girder),
            ("Box girder", "bridge_style_box", BridgeStyle::BoxGirder),
            ("Semi-arch", "bridge_style_arch", BridgeStyle::Arch),
            ("Suspension", "bridge_style_suspension", BridgeStyle::Suspension),
        ] {
            if ui.selectable_label(style == st, label).on_hover_text(format!("RoadArchitect {label} kit — piers/deep box/rib+s spandrels/towers+cables")).clicked() {
                let _ = actions::perform(app, action);
                ui.close_menu();
            }
        }
        ui.separator();
        bridge_toggle(app, ui, "Tunnel (half-cylinder vault)", tunnel, "toggle_tunnel");
        bridge_toggle(app, ui, "Guardrails (W-beam)", guardrail, "toggle_guardrail");
        bridge_toggle(app, ui, "Cable barrier (median)", cable, "toggle_cable_barrier");
    });
}

fn bridge_toggle(app: &mut RoadkitApp, ui: &mut egui::Ui, label: &str, on: bool, action: &str) {
    let mut v = on;
    if ui.checkbox(&mut v, label).on_hover_text("Toggles the flag on the selected RoadContainer and rebuilds").clicked() {
        let _ = actions::perform(app, action);
        ui.close_menu();
    }
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

                // right side: camera buttons + primary Run
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
        for (tab, label) in [(Tab::Plan, "2D Plan — snapping & junctions"), (Tab::Scene3d, "3D Scene — roads · bridges · GLB dressing")] {
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
