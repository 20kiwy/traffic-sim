//! The right dock — VectorCraft-style closable panels:
//! icon column (38 px) · dock (300 px, tab strip 33) ·
//! Layers (the data-driven layer engine) · Properties · Assets · Script · Console.

use crate::actions;
use crate::state::{DockTab, MAX_LANES, RoadkitApp, ScriptStatus, Tool};
use egui::{Color32, RichText, ScrollArea};
use egui_kit::IconName;
use egui_kit::{eye_lock_columns, search_field, tree_row, TreeRowStyle};
use egui_kit::tokens;
use roads::model::{Alignment, LaneDir, TrafficUpdate};

pub const ICON_COL: f32 = 46.0;
pub const DOCK_DEFAULT: f32 = 300.0;
pub const TAB_STRIP_H: f32 = 33.0;

/* ------------------------------------------------------------------ */
/* Icon column — panel visibility toggles (far right, always visible)  */
/* ------------------------------------------------------------------ */

pub fn icon_column(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = tokens(ctx);
    egui::SidePanel::right("dock_icon_column")
        .exact_width(ICON_COL)
        .resizable(false)
        .frame(egui::Frame::none().fill(t.app_bar))
        .show(ctx, |ui| {
            egui_kit::card(
                ui,
                egui::Margin { left: 3.0, right: 3.0, top: 6.0, bottom: 6.0 },
                |ui| {
                    ui.set_width(ui.available_width());
                    for tab in DockTab::ALL {
                        let open = app.panels.is_open(tab);
                        let active = open && app.dock_tab == tab && !app.dock_collapsed;
                        let resp = egui_kit::icon_button_caption(
                            ui,
                            tab.icon(),
                            active,
                            tab.caption(),
                            tab.label(),
                        );
                        if resp.clicked() {
                            if open && app.dock_tab == tab && !app.dock_collapsed {
                                app.panels.set_open(tab, false); // second click closes
                            } else {
                                app.panels.set_open(tab, true);
                                app.dock_tab = tab;
                                app.dock_collapsed = false;
                            }
                        }
                    }
                },
            );
        });
}

/* ------------------------------------------------------------------ */
/* The dock — tab strip + active panel body                            */
/* ------------------------------------------------------------------ */

pub fn dock(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = tokens(ctx);
    if app.dock_collapsed || app.panels.open_tabs().is_empty() {
        return;
    }
    // keep the active tab valid
    if !app.panels.is_open(app.dock_tab) {
        app.dock_tab = app.panels.open_tabs()[0];
    }
    egui::SidePanel::right("dock")
        // FIXED width — the old resizable panel drifted wider after drags
        // (felt like "the panel grows and the app is stuck"). VectorCraft's
        // dock is a fixed 300 px column with a collapse toggle, so we match it.
        .exact_width(DOCK_DEFAULT)
        .resizable(false)
        .frame(egui::Frame::none().fill(t.app_bar))
        .show(ctx, |ui| {
            egui_kit::card(
                ui,
                egui::Margin { left: 6.0, right: 6.0, top: 6.0, bottom: 6.0 },
                |ui| {
                    dock_tab_strip(app, ui);
                    ui.add_space(4.0);
                    egui::ScrollArea::vertical()
                        .id_source("dock_body")
                        .auto_shrink([false, false])
                        .show(ui, |ui| match app.dock_tab {
                            DockTab::Layers => layers_content(app, ui),
                            DockTab::Properties => properties_content(app, ui),
                            DockTab::Assets => assets_content(app, ui),
                            DockTab::Script => script_content(app, ui),
                            DockTab::Console => console_content(app, ui),
                            DockTab::History => history_content(app, ui),
                        });
                },
            );
        });
}

/// The `Properties | Layers | Libraries | Tiles` header from the reference
/// screenshot: pill tabs on a tab-strip background + » collapse.
fn dock_tab_strip(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), TAB_STRIP_H), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, t.tab_strip);
    let open = app.panels.open_tabs();
    let mut x = rect.left() + 6.0;
    for tab in &open {
        let selected = *tab == app.dock_tab;
        let label_w = 34.0 + tab.label().len() as f32 * 6.4;
        let pill = egui::Rect::from_min_size(egui::pos2(x, rect.top() + 4.0), egui::vec2(label_w, TAB_STRIP_H - 8.0));
        let resp = ui.interact(pill, egui::Id::new(("dock_tab", *tab as u8)), egui::Sense::click());
        if selected {
            ui.painter().rect_filled(pill.shrink2(egui::vec2(3.0, 5.0)).shrink(0.0), 6.0, t.button);
            ui.painter().rect_filled(
                pill.shrink2(egui::vec2(3.0, 5.0)),
                6.0,
                t.button,
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(pill.shrink2(egui::vec2(3.0, 5.0)), 6.0, t.hover.gamma_multiply(0.5));
        }
        ui.painter().text(
            pill.center(),
            egui::Align2::CENTER_CENTER,
            tab.label(),
            egui::FontId::proportional(if selected { 12.5 } else { 12.0 }),
            if selected { t.text_strong } else { t.text_dim },
        );
        if resp.clicked() {
            app.dock_tab = *tab;
        }
        x += label_w + 2.0;
    }
    // collapse toggle at the right edge
    let cbtn = egui::Rect::from_min_size(egui::pos2(rect.right() - 22.0, rect.top() + 8.0), egui::vec2(16.0, 16.0));
    let cr = ui.interact(cbtn, egui::Id::new("dock_collapse"), egui::Sense::click());
    egui_kit::paint(ui, IconName::ChevronsRight, cbtn.shrink(1.0), if cr.hovered() { t.text } else { t.text_dim });
    if cr.clicked() {
        app.dock_collapsed = true;
    }
}

/* ================================================================== */
/* LAYERS — the data-driven layer engine (RoadManager→Container→Point) */
/* ================================================================== */

fn layer_matches(search: &str, name: &str) -> bool {
    search.is_empty() || name.to_lowercase().contains(&search.to_lowercase())
}

fn layers_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_sized(egui::vec2(ui.available_width() - 30.0, 24.0), |ui: &mut egui::Ui| search_field(ui, "layer_search", &mut app.layer_search));
        if egui_kit::icon_button_flat(ui, IconName::Search, false, "Search layers").clicked() {
            // focus behaviour handled by the field itself
        }
    });
    ui.add_space(3.0);

    let data = {
        let sc = app.scene.borrow();
        (
            sc.curves
                .iter()
                .map(|c| (c.name.clone(), c.visible, c.color, c.points.len(), c.points.iter().map(|p| (p.name.clone(), p.visible, p.locked)).collect::<Vec<_>>()))
                .collect::<Vec<_>>(),
            sc.junctions
                .iter()
                .map(|j| (j.kind.label().to_string(), j.legs.len(), j.x, j.z))
                .collect::<Vec<_>>(),
            sc.props.len(),
        )
    };
    let (curves, junctions, nprops) = data;

    // ---- RoadManager root row ----
    let total_pts: usize = curves.iter().map(|c| c.3).sum();
    let root_sel = app.sel.is_none() && app.sel_container.is_none() && app.sel_junction.is_none();
    let _ = tree_row(
        ui,
        0,
        None,
        &format!("RoadManager — {total_pts} pts · {} props", nprops),
        &TreeRowStyle { row_h: 24.0, selected: root_sel, icon: Some(IconName::Junction), ..Default::default() },
        |ui, _r| {
            // trailing spacer
            let _ = ui;
        },
    )
    .clicked()
    .then_some(());

    // ---- container rows ----
    for (ci, (name, cvis, color, npts, points)) in curves.iter().enumerate() {
        if !layer_matches(&app.layer_search, name) && points.iter().all(|(pn, _, _)| !layer_matches(&app.layer_search, pn)) {
            continue;
        }
        let open = app.expanded.contains(name) || !app.layer_search.is_empty();
        let sel = app.sel_container == Some(ci) && app.sel.is_none();
        let col = Color32::from_rgb(color[0], color[1], color[2]);
        let row = tree_row(
            ui,
            0,
            Some(open),
            &format!("{name}   {npts} pts"),
            &TreeRowStyle {
                row_h: 26.0,
                selected: sel,
                colour_bar: Some(col),
                bar_w: 4.0,
                icon: Some(IconName::Road),
                ..Default::default()
            },
            |ui, _r| {
                let _ = ui;
            },
        );
        // eye/lock columns at the right edge
        let row_rect = row.rect;
        let (eye_click, lock_click) = eye_lock_columns(ui, &row_rect, *cvis, false);
        if eye_click {
            let mut sc = app.scene.borrow_mut();
            sc.curves[ci].visible = !sc.curves[ci].visible;
            sc.dirty = true;
            let v = sc.curves[ci].visible;
            let nm = sc.curves[ci].name.clone();
            sc.log(format!("[layers] '{nm}' visible = {v}"));
            drop(sc);
            app.render.dirty = true;
        }
        if lock_click {
            let mut sc = app.scene.borrow_mut();
            let lock = !sc.curves[ci].points.iter().all(|p| p.locked);
            let cname = sc.curves[ci].name.clone();
            for p in sc.curves[ci].points.iter_mut() {
                p.locked = lock;
            }
            sc.log(format!("[layers] '{cname}' points locked = {lock}"));
            drop(sc);
        }
        if row.clicked() {
            app.clear_selection();
            app.sel_container = Some(ci);
            app.dock_tab = DockTab::Properties;
        }
        // disclosure toggle on the chevron (click without selecting? — treat
        // arrow key / alt-click; also expand when selected)
        if row.secondary_clicked() {
            if app.expanded.contains(name) {
                app.expanded.remove(name);
            } else {
                app.expanded.insert(name.clone());
            }
        }
        if sel && !app.expanded.contains(name) && *npts > 0 && app.layer_search.is_empty() {
            // auto-expand the selected container (layers panel parity with Godot)
        }

        // ---- point rows ----
        if open {
            for (pi, (pname, pvis, plock)) in points.iter().enumerate() {
                if !layer_matches(&app.layer_search, pname) && !layer_matches(&app.layer_search, name) {
                    continue;
                }
                let label = if pname.is_empty() { format!("RP_{:03}", pi + 1) } else { pname.clone() };
                let psel = app.sel == Some((ci, pi));
                let prow = tree_row(
                    ui,
                    1,
                    None,
                    &label,
                    &TreeRowStyle { row_h: 22.0, selected: psel, icon: Some(IconName::Point), ..Default::default() },
                    |ui, _r| {
                        let _ = ui;
                    },
                );
                let pr = prow.rect;
                let (peye, plock_click) = eye_lock_columns(ui, &pr, *pvis, *plock);
                if peye {
                    let mut sc = app.scene.borrow_mut();
                    sc.curves[ci].points[pi].visible = !sc.curves[ci].points[pi].visible;
                    sc.dirty = true;
                    drop(sc);
                    app.render.dirty = true;
                }
                if plock_click {
                    let mut sc = app.scene.borrow_mut();
                    sc.curves[ci].points[pi].locked = !sc.curves[ci].points[pi].locked;
                    drop(sc);
                }
                if prow.clicked() {
                    app.clear_selection();
                    app.sel = Some((ci, pi));
                    app.dock_tab = DockTab::Properties;
                }
            }
            // "add point" hint row
            if app.layer_search.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(34.0);
                    if ui
                        .add(
                            egui::Button::new(RichText::new("+ point").size(10.5).color(t.text_dim))
                                .fill(Color32::TRANSPARENT)
                                .min_size(egui::vec2(70.0, 18.0)),
                        )
                        .on_hover_text("Append a RoadPoint to this container (or use the Add tool on the plan)")
                        .clicked()
                    {
                        let last = app
                            .scene
                            .borrow()
                            .curves
                            .get(ci)
                            .and_then(|c| c.points.last())
                            .map(|p| (p.x + 40.0, p.z))
                            .unwrap_or((app.plan.cx, app.plan.cz));
                        app.sel = Some((ci, 0));
                        app.sel_container = None;
                        app.add_point_at(last.0, last.1);
                        app.dock_tab = DockTab::Properties;
                    }
                });
            }
        }
    }

    // ---- junction rows ----
    if !junctions.is_empty() {
        ui.add_space(4.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
        ui.painter().text(
            egui::pos2(rect.left() + 6.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            "ROAD INTERSECTIONS",
            egui::FontId::proportional(10.0),
            t.text_dim,
        );
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], egui::Stroke::new(1.0, t.divider));
        for (ji, (kind, legs, x, z)) in junctions.iter().enumerate() {
            if !layer_matches(&app.layer_search, kind) {
                continue;
            }
            let sel = app.sel_junction == Some(ji);
            let row = tree_row(
                ui,
                0,
                None,
                &format!("{kind} · {legs} legs @ ({x:.0}, {z:.0})"),
                &TreeRowStyle { row_h: 24.0, selected: sel, icon: Some(IconName::Junction), ..Default::default() },
                |ui, _r| {
                    let _ = ui;
                },
            );
            if row.clicked() {
                app.clear_selection();
                app.sel_junction = Some(ji);
                app.dock_tab = DockTab::Properties;
            }
        }
    }

    // ---- bottom bar ----
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::hover());
    ui.painter().line_segment([rect.left_top(), rect.right_top()], egui::Stroke::new(1.0, t.divider));
    ui.painter().text(
        egui::pos2(rect.left() + 6.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        format!("{} roads · {total_pts} points · {} junctions", curves.len(), junctions.len()),
        egui::FontId::proportional(11.0),
        t.text_dim,
    );
    let trash = egui::Rect::from_min_size(egui::pos2(rect.right() - 26.0, rect.top() + 2.0), egui::vec2(22.0, 22.0));
    let tr = ui.interact(trash, egui::Id::new("layers_trash"), egui::Sense::click());
    egui_kit::paint(ui, IconName::Trash, trash.shrink(3.0), if tr.hovered() { t.text } else { t.text_dim });
    if tr.clicked() {
        if let Some((ci, pi)) = app.sel {
            let _ = pi;
            app.delete_selected_point();
        } else if let Some(ci) = app.sel_container {
            app.delete_container(ci);
        }
    }
    let plus = egui::Rect::from_min_size(egui::pos2(rect.right() - 52.0, rect.top() + 2.0), egui::vec2(22.0, 22.0));
    let pr = ui.interact(plus, egui::Id::new("layers_plus"), egui::Sense::click());
    egui_kit::paint(ui, IconName::Plus, plus.shrink(3.0), if pr.hovered() { t.text } else { t.text_dim });
    if pr.clicked() {
        app.add_road();
    }
    tr.on_hover_text("Delete the selected RoadPoint / RoadContainer");
    pr.on_hover_text("New RoadContainer with 2 points");
}

/* ================================================================== */
/* PROPERTIES — sectioned inspector (point / container / junction)     */
/* ================================================================== */

fn divider(ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    ui.add_space(3.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), egui::Sense::hover());
    ui.painter().line_segment([rect.left_center(), rect.right_center()], egui::Stroke::new(1.0, t.divider));
    ui.add_space(3.0);
}

fn header(ui: &mut egui::Ui, label: &str) {
    let t = tokens(ui.ctx());
    ui.label(RichText::new(label).strong().size(12.5).color(t.text_strong));
}

fn properties_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    if let Some(ji) = app.sel_junction {
        junction_inspector(app, ui, ji);
        return;
    }
    if let Some((ci, pi)) = app.sel {
        point_inspector(app, ui, ci, pi);
        return;
    }
    if let Some(ci) = app.sel_container {
        container_inspector(app, ui, ci);
        return;
    }
    scene_inspector(app, ui);
}

/* ---- RoadPoint inspector (the Godot "Edit RoadPoint" panel) ------- */

/// RP_### display name like the plugin when the node name is empty.
fn point_display_name(pt: &roads::model::RoadPoint, pi: usize) -> String {
    if pt.name.is_empty() {
        format!("RP_{:03}", pi + 1)
    } else {
        pt.name.clone()
    }
}

/// Small rounded chip with coloured text (traffic_dir / lane types).
fn chip(ui: &mut egui::Ui, text: &str, col: Color32) {
    let t = tokens(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(egui::vec2(20.0, 16.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 4.0, t.input);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(10.5),
        col,
    );
}

/// One `+`/`−` lane button (disabled = GRG rule refuses the op).
fn lane_btn(ui: &mut egui::Ui, label: &str, enabled: bool) -> egui::Response {
    let btn = egui::Button::new(RichText::new(label).strong().size(13.0))
        .min_size(egui::vec2(24.0, 24.0));
    if enabled {
        ui.add(btn)
    } else {
        ui.add_enabled(false, btn)
    }
}

/// Surface an action helper's refusal in the console + status bar.
fn show_action_result(app: &mut RoadkitApp, res: Result<(), String>) {
    if let Err(e) = res {
        app.scene.borrow_mut().log(format!("[edit] refused — {e}"));
        app.status = e;
    }
}

fn point_inspector(app: &mut RoadkitApp, ui: &mut egui::Ui, ci: usize, pi: usize) {
    let t = tokens(ui.ctx());
    let valid = {
        let sc = app.scene.borrow();
        ci < sc.curves.len() && pi < sc.curves[ci].points.len()
    };
    if !valid {
        egui_kit::dim_text(ui, "Selection out of range — scene changed?");
        return;
    }
    let cname = app.scene.borrow().curves[ci].name.clone();
    let mut pt = app.scene.borrow().curves[ci].points[pi].clone();
    let pname = point_display_name(&pt, pi);
    let mut changed = false;
    // any history-routed helper (lane row / copy-apply / flip / select)
    // mutates the scene directly — skip the drag-form write-back that frame
    let mut handled = false;

    // header: "Edit RoadPoint" + RP_### display name
    ui.horizontal(|ui| {
        header(ui, "Edit RoadPoint");
        ui.label(RichText::new(&pname).size(11.5).color(t.text_dim));
    });
    egui_kit::dim_text(
        ui,
        &format!(
            "in '{cname}' · {} · {}",
            if pt.dirt { "dirt surface" } else { "paved" },
            if pt.oneway { "one-way" } else { "two-way" }
        ),
    );
    ui.add_space(2.0);

    if egui_kit::btn(ui, "Select Next RoadPoint", false)
        .on_hover_text("Walk the selection to the next RoadPoint (wraps at the ends)")
        .clicked()
    {
        app.select_next_point();
        handled = true;
    }
    ui.add_space(2.0);

    // the GRG lane row: [ + - || - + ]
    let (lanes, rev, fwd) = (pt.lanes, pt.rev_lanes.min(pt.lanes), pt.fwd_lanes());
    // bulk = the checkbox OR a held shift key (GRG's shift-click behaviour;
    // shift is transient — only the checkbox latches across frames)
    let shift_held = ui.input(|i| i.modifiers.shift);
    let mut bulk_mem = ui
        .memory(|m| m.data.get_temp::<bool>(egui::Id::new("rp_apply_all")).unwrap_or(false));
    let bulk = bulk_mem || shift_held;
    ui.horizontal(|ui| {
        if lane_btn(ui, "+", lanes < MAX_LANES)
            .on_hover_text(
                "Add Reverse lane.\nHold shift to add another reverse lane to all RoadPoints in this container.",
            )
            .clicked()
        {
            let r = app.update_point_traffic(ci, pi, TrafficUpdate::AddReverse, bulk);
            show_action_result(app, r);
            handled = true;
        }
        if lane_btn(ui, "−", lanes > 1 && rev > 0)
            .on_hover_text(
                "Remove Reverse lane.\nHold shift to remove a reverse lane from all RoadPoints in this container.",
            )
            .clicked()
        {
            let r = app.update_point_traffic(ci, pi, TrafficUpdate::RemReverse, bulk);
            show_action_result(app, r);
            handled = true;
        }
        ui.label(
            RichText::new("||")
                .strong()
                .size(14.0)
                .color(Color32::from_rgb(0xf3, 0xbb, 0x11)),
        )
        .on_hover_text("the lane divider");
        if lane_btn(ui, "−", lanes > 1 && fwd > 0)
            .on_hover_text("Remove Forward lane.")
            .clicked()
        {
            let r = app.update_point_traffic(ci, pi, TrafficUpdate::RemForward, bulk);
            show_action_result(app, r);
            handled = true;
        }
        if lane_btn(ui, "+", lanes < MAX_LANES)
            .on_hover_text("Add Forward lane.")
            .clicked()
        {
            let r = app.update_point_traffic(ci, pi, TrafficUpdate::AddForward, bulk);
            show_action_result(app, r);
            handled = true;
        }
        ui.label(
            RichText::new(format!("{lanes} lanes"))
                .size(10.5)
                .color(t.text_dim),
        );
    });
    ui.checkbox(&mut bulk_mem, "apply to whole container (bulk)").on_hover_text(
        "Lane buttons then edit EVERY RoadPoint of this container (the shift-click bulk behaviour)",
    );
    ui.memory_mut(|m| m.data.insert_temp(egui::Id::new("rp_apply_all"), bulk_mem));
    ui.add_space(1.0);

    if egui_kit::btn(ui, "Select Prior RoadPoint", false)
        .on_hover_text("Walk the selection to the previous RoadPoint (wraps at the ends)")
        .clicked()
    {
        app.select_prior_point();
        handled = true;
    }
    ui.add_space(3.0);

    // Settings: [Copy Settings][Apply] (+[Apply All] with the bulk checkbox on)
    let has_clip = app.point_clipboard.is_some();
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Copy Settings", false)
            .on_hover_text("Copy this RoadPoint's cross-section settings")
            .clicked()
        {
            let r = app.copy_point_settings(ci, pi);
            show_action_result(app, r);
            handled = true;
        }
        let apply_btn = egui::Button::new(RichText::new("Apply").size(11.5));
        let resp = if has_clip { ui.add(apply_btn) } else { ui.add_enabled(false, apply_btn) };
        if resp
            .on_hover_text("Apply the copied settings onto this RoadPoint")
            .clicked()
        {
            let r = app.apply_point_settings(ci, pi, false);
            show_action_result(app, r);
            handled = true;
        }
        if bulk {
            let apply_all = egui::Button::new(RichText::new("Apply All").size(11.5));
            let resp = if has_clip { ui.add(apply_all) } else { ui.add_enabled(false, apply_all) };
            if resp
                .on_hover_text("Apply the copied settings onto EVERY RoadPoint of this container")
                .clicked()
            {
                let r = app.apply_point_settings(ci, pi, true);
                show_action_result(app, r);
                handled = true;
            }
        }
    });
    if egui_kit::btn(ui, "Flip", false)
        .on_hover_text("Flip RoadPoint — swap prior/next handles, shoulders and the travel direction")
        .clicked()
    {
        let r = app.flip_point(ci, pi);
        show_action_result(app, r);
        handled = true;
    }
    ui.add_space(3.0);

    /* ---- collapsing property groups (Godot RoadPoint editor) ---- */

    // LANES (open by default)
    egui::CollapsingHeader::new(RichText::new("Lanes").size(12.0).color(t.text_strong))
        .id_source("pt_lanes")
        .default_open(true)
        .show(ui, |ui| {
            if ui
                .checkbox(&mut pt.auto_lanes, "auto_lanes")
                .on_hover_text("Automatically assign lane types (trim sheet columns) from traffic_dir")
                .changed()
            {
                changed = true;
            }
            ui.horizontal(|ui| {
                ui.label(RichText::new("traffic_dir").size(11.5).color(t.text_dim));
                for d in pt.traffic_dir() {
                    match d {
                        LaneDir::Reverse => chip(ui, "R", Color32::from_rgb(0xe8, 0x6a, 0x52)),
                        LaneDir::Forward => chip(ui, "F", Color32::from_rgb(0x63, 0xcf, 0x8e)),
                        _ => chip(ui, "B", Color32::from_rgb(0xf3, 0xbb, 0x11)),
                    }
                }
            });
            egui::Grid::new("pt_lanes_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                let mut rev = pt.rev_lanes as i64;
                ui.label(RichText::new("rev_lanes").size(11.5));
                if ui.add(egui::DragValue::new(&mut rev).range(0..=(MAX_LANES as i64 - 1))).changed() {
                    pt.rev_lanes = (rev.max(0) as usize).min(pt.lanes.saturating_sub(1));
                    pt.oneway = pt.rev_lanes == 0;
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("lanes (total)").size(11.5));
                ui.label(RichText::new(format!("{}", pt.lanes)).size(11.5).color(t.text_dim));
                ui.end_row();
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("lane types").size(11.5).color(t.text_dim));
                for lt in pt.lane_types() {
                    chip(ui, lt, Color32::from_rgb(0x9d, 0xd3, 0xa0));
                }
            });
            egui_kit::dim_text(
                ui,
                if pt.auto_lanes {
                    "lane types are auto-assigned from traffic_dir (auto_lanes)"
                } else {
                    "auto_lanes off — lane types keep the current assignment"
                },
            );
        });

    // ROAD GENERATION (open by default)
    egui::CollapsingHeader::new(RichText::new("Road Generation").size(12.0).color(t.text_strong))
        .id_source("pt_roadgen")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("pt_gen_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("prior_mag").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.prior_mag).speed(0.5).suffix(" m").range(0.5..=500.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("next_mag").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.next_mag).speed(0.5).suffix(" m").range(0.5..=500.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("create_geo").size(11.5));
                if ui.checkbox(&mut pt.create_geo, "").on_hover_text("Generate the procedural road geometry from this point").changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("flatten_terrain").size(11.5));
                if ui.checkbox(&mut pt.flatten_terrain, "").on_hover_text("Flatten the terrain under segments touching this point").changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("lane_width").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.lane_width).speed(0.1).suffix(" m").range(1.0..=8.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("shoulder_width_l").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.shoulder_l).speed(0.1).suffix(" m").range(0.0..=8.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("shoulder_width_r").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.shoulder_r).speed(0.1).suffix(" m").range(0.0..=8.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                let mut gx = pt.gutter_profile.0;
                ui.label(RichText::new("gutter_profile_x").size(11.5));
                if ui.add(egui::DragValue::new(&mut gx).speed(0.1).range(0.0..=6.0)).changed() {
                    pt.gutter_profile.0 = gx;
                    changed = true;
                }
                ui.end_row();
                let mut gy = pt.gutter_profile.1;
                ui.label(RichText::new("gutter_profile_y").size(11.5));
                if ui.add(egui::DragValue::new(&mut gy).speed(0.1).range(-3.0..=0.0)).changed() {
                    pt.gutter_profile.1 = gy;
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("alignment").size(11.5));
                let mut al = match pt.alignment {
                    Alignment::Geometric => 0usize,
                    Alignment::Divider => 1,
                };
                egui::ComboBox::from_id_source("pt_alignment")
                    .selected_text(["Geometric", "Divider"][al])
                    .width(120.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut al, 0, "Geometric");
                        ui.selectable_value(&mut al, 1, "Divider");
                    })
                    .response
                    .on_hover_text("Geometric centers the road on the origin; Divider centers the lane divider");
                pt.alignment = if al == 1 { Alignment::Divider } else { Alignment::Geometric };
                ui.end_row();
                ui.label(RichText::new("underside_thickness").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.underside_thickness).speed(0.1).suffix(" m"))
                    .on_hover_text("-1 = use the container / manager value")
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();
            });
        });

    // INTERNAL DATA (closed)
    egui::CollapsingHeader::new(RichText::new("Internal data").size(12.0).color(t.text_strong))
        .id_source("pt_internal")
        .default_open(false)
        .show(ui, |ui| {
            let (prior_name, next_name) = {
                let sc = app.scene.borrow();
                let pts = &sc.curves[ci].points;
                let nm = |idx: usize| {
                    pts.get(idx)
                        .map(|q| point_display_name(q, idx))
                        .unwrap_or_else(|| "(none)".into())
                };
                (
                    if pi > 0 { nm(pi - 1) } else { "(none)".to_string() },
                    nm(pi + 1),
                )
            };
            ui.horizontal(|ui| {
                ui.label(RichText::new("prior point").size(11.5).color(t.text_dim));
                ui.label(RichText::new(prior_name).size(11.5));
            });
            ui.horizontal(|ui| {
                ui.label(RichText::new("next point").size(11.5).color(t.text_dim));
                ui.label(RichText::new(next_name).size(11.5));
            });
            if ui
                .checkbox(&mut pt.terminated, "terminated")
                .on_hover_text("Mark as a non-edge end — excluded from endpoint joins")
                .changed()
            {
                changed = true;
            }
        });

    // TRANSFORM (closed)
    egui::CollapsingHeader::new(RichText::new("Transform").size(12.0).color(t.text_strong))
        .id_source("pt_transform_hdr")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("pt_transform").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("x (east)").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.x).speed(0.5).suffix(" m")).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("z (south)").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.z).speed(0.5).suffix(" m")).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("elevation").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.elev).speed(0.1).suffix(" m")).changed() {
                    changed = true;
                }
                ui.end_row();
                let mut bearing_deg = pt.bearing.to_degrees();
                ui.label(RichText::new("bearing").size(11.5));
                if ui
                    .add(egui::DragValue::new(&mut bearing_deg).speed(1.0).suffix(" °").range(-360.0..=360.0))
                    .changed()
                {
                    pt.bearing = bearing_deg.to_radians();
                    changed = true;
                }
                ui.end_row();
            });
        });

    // CROSS-SECTION EXTRAS (closed)
    egui::CollapsingHeader::new(RichText::new("Cross-section extras").size(12.0).color(t.text_strong))
        .id_source("pt_cross_hdr")
        .default_open(false)
        .show(ui, |ui| {
            egui::Grid::new("pt_cross").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("median").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.median).speed(0.2).suffix(" m").range(0.0..=12.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("pathway L").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.pathway_l).speed(0.2).suffix(" m").range(0.0..=8.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("pathway R").size(11.5));
                if ui.add(egui::DragValue::new(&mut pt.pathway_r).speed(0.2).suffix(" m").range(0.0..=8.0)).changed() {
                    changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("dirt surface").size(11.5));
                if ui.checkbox(&mut pt.dirt, "").on_hover_text("Mud lanes + rough gravel shoulders").changed() {
                    changed = true;
                }
                ui.end_row();
            });
            ui.horizontal(|ui| {
                if egui_kit::btn(ui, "BRT Mwendokasi median", true).clicked() {
                    pt.median = 3.2;
                    pt.pathway_l = 2.5;
                    pt.pathway_r = 2.5;
                    pt.lanes = pt.lanes.max(4);
                    pt.lane_width = 3.5;
                    changed = true;
                    app.scene.borrow_mut().log("[edit] BRT Mwendokasi preset — 3.2 m cement median + tactile edges + 2.5 m pathways");
                }
                if egui_kit::btn(ui, "Sidewalks", false).clicked() {
                    pt.pathway_l = 2.0;
                    pt.pathway_r = 2.0;
                    pt.median = pt.median.min(2.0);
                    changed = true;
                }
            });
        });

    // write-back (skipped when a history-routed helper already mutated)
    if changed && !handled {
        let mut ok = false;
        {
            let mut sc = app.scene.borrow_mut();
            if ci < sc.curves.len() && pi < sc.curves[ci].points.len() {
                sc.curves[ci].points[pi] = pt.clone();
                sc.dirty = true;
                ok = true;
            }
        }
        if ok {
            app.render.dirty = true;
        }
    }

    // schema hints
    if let Some(def) = app.schema.class("RoadPoint") {
        egui::CollapsingHeader::new(RichText::new(format!("schema · RoadPoint ({} props)", def.props.len())).size(10.5))
            .show(ui, |ui| {
                for p in &def.props {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("{} : {}", p.name, p.kind.label())).size(9.5).color(tokens(ui.ctx()).text_dim));
                        if !p.hint.is_empty() {
                            ui.label(RichText::new(p.hint.clone()).size(9.0).color(tokens(ui.ctx()).text_disabled));
                        }
                    });
                }
            });
    }

    ui.add_space(3.0);
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Reverse", false).on_hover_text("Reverse the whole curve").clicked() {
            handled = true;
            let mut sc = app.scene.borrow_mut();
            let c = &mut sc.curves[ci];
            c.points.reverse();
            for p in c.points.iter_mut() {
                p.bearing += std::f64::consts::PI;
                std::mem::swap(&mut p.prior_mag, &mut p.next_mag);
            }
            sc.dirty = true;
            drop(sc);
            app.render.dirty = true;
        }
        if egui_kit::btn(ui, "Smooth", false).on_hover_text("Point handles at neighbour thirds").clicked() {
            handled = true;
            let mut sc = app.scene.borrow_mut();
            let c = &mut sc.curves[ci];
            let n = c.points.len();
            if n >= 2 {
                let pts = c.points.clone();
                for i in 0..n {
                    let prev = &pts[i.saturating_sub(1)];
                    let next = &pts[(i + 1).min(n - 1)];
                    let dx = next.x - prev.x;
                    let dz = next.z - prev.z;
                    if dx.hypot(dz) > 1e-9 {
                        c.points[i].bearing = dz.atan2(dx);
                    }
                    let d = ((next.x - c.points[i].x).powi(2) + (next.z - c.points[i].z).powi(2)).sqrt() / 3.0;
                    c.points[i].next_mag = d.max(2.0);
                    c.points[i].prior_mag = d.max(2.0);
                }
                sc.dirty = true;
            }
            drop(sc);
            app.render.dirty = true;
        }
        if egui_kit::btn(ui, "Delete", false).clicked() {
            handled = true;
            app.delete_selected_point();
        }
    });
}

/* ---- RoadContainer inspector ------------------------------------- */

const LAYER_COLORS: [[u8; 3]; 8] = [
    [0x3f, 0x8e, 0x5a],
    [0xc2, 0x4b, 0x3d],
    [0x3d, 0x7b, 0xc2],
    [0xc2, 0x94, 0x3d],
    [0x8a, 0x5a, 0xc2],
    [0x3d, 0xb5, 0xc2],
    [0xc2, 0x5a, 0x8f],
    [0x77, 0x87, 0x99],
];

fn container_inspector(app: &mut RoadkitApp, ui: &mut egui::Ui, ci: usize) {
    if ci >= app.scene.borrow().curves.len() {
        egui_kit::dim_text(ui, "Selection out of range — scene changed?");
        return;
    }
    header(ui, "RoadContainer");
    let (name, closed, visible, color, npts, len_m) = {
        let sc = app.scene.borrow();
        let c = &sc.curves[ci];
        (c.name.clone(), c.closed, c.visible, c.color, c.points.len(), c.length_m())
    };
    egui_kit::dim_text(ui, &format!("{npts} RoadPoints · {:.0} m · {}", len_m, if closed { "closed loop" } else { "open" }));

    ui.add_space(2.0);
    let mut new_name = name.clone();
    ui.label(RichText::new("name").size(11.5));
    if ui
        .add(egui::TextEdit::singleline(&mut new_name).desired_width(ui.available_width()))
        .lost_focus()
        && new_name != name
        && !new_name.is_empty()
    {
        app.scene.borrow_mut().curves[ci].name = new_name.clone();
        app.scene.borrow_mut().dirty = true;
        app.scene.borrow_mut().log(format!("[edit] container renamed to '{new_name}'"));
    }

    let mut vis = visible;
    if egui_kit::checkbox_row(ui, "visible in plan + 3D", &mut vis).changed() {
        app.scene.borrow_mut().curves[ci].visible = vis;
        app.render.dirty = true;
    }
    let mut cls = closed;
    if egui_kit::checkbox_row(ui, "closed loop", &mut cls).changed() {
        app.scene.borrow_mut().curves[ci].closed = cls;
        app.render.dirty = true;
    }

    ui.add_space(3.0);
    ui.label(RichText::new("layer colour").size(11.5));
    ui.horizontal(|ui| {
        for c in LAYER_COLORS {
            let col = Color32::from_rgb(c[0], c[1], c[2]);
            let resp = egui_kit::color_swatch(ui, col, 18.0);
            if resp.clicked() {
                app.scene.borrow_mut().curves[ci].color = c;
            }
            if color == c {
                let t = tokens(ui.ctx());
                let r = resp.rect;
                ui.painter().rect_stroke(r.expand(2.0), 3.0, egui::Stroke::new(1.2, t.accent));
            }
        }
    });

    /* ---- GRG RoadContainer property groups ---- */
    let t = tokens(ui.ctx());
    let mut c = app.scene.borrow().curves[ci].clone();
    let mut cchanged = false;

    egui::CollapsingHeader::new(RichText::new("Road Generation").size(12.0).color(t.text_strong))
        .id_source("cont_roadgen")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("cont_gen_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("create_geo").size(11.5));
                if ui.checkbox(&mut c.create_geo, "").on_hover_text("Generate the procedural road geometry for this container").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("density").size(11.5));
                if ui.add(egui::DragValue::new(&mut c.density).speed(0.25).suffix(" m"))
                    .on_hover_text("Loop-cut spacing; -1 = use the RoadManager value")
                    .changed()
                {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("use_lowpoly_preview").size(11.5));
                if ui.checkbox(&mut c.use_lowpoly_preview, "").on_hover_text("Fewer loop cuts while dragging").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("flatten_terrain").size(11.5));
                if ui.checkbox(&mut c.flatten_terrain, "").on_hover_text("Flatten the terrain under this container's roads").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("underside_thickness").size(11.5));
                if ui.add(egui::DragValue::new(&mut c.underside_thickness).speed(0.1).suffix(" m"))
                    .on_hover_text("-1 = use the manager value")
                    .changed()
                {
                    cchanged = true;
                }
                ui.end_row();
            });
            if c.density < 0.0 {
                egui_kit::dim_text(ui, "density follows the RoadManager (see the Layers root row)");
            }
        });

    egui::CollapsingHeader::new(RichText::new("Lanes and AI").size(12.0).color(t.text_strong))
        .id_source("cont_lanes")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("cont_lanes_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("generate_ai_lanes").size(11.5));
                if ui.checkbox(&mut c.generate_ai_lanes, "").on_hover_text("Auto-generate RoadLanes for AI agents").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("auto_free_vehicles").size(11.5));
                if ui.checkbox(&mut c.auto_free_vehicles, "").on_hover_text("Free agents registered on a lane when it exits the tree").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("draw_lanes_editor").size(11.5));
                if ui.checkbox(&mut c.draw_lanes_editor, "").on_hover_text("Visualize lanes + directions in the editor").changed() {
                    cchanged = true;
                }
                ui.end_row();
                ui.label(RichText::new("draw_lanes_game").size(11.5));
                if ui.checkbox(&mut c.draw_lanes_game, "").on_hover_text("Visualize lanes + directions at runtime").changed() {
                    cchanged = true;
                }
                ui.end_row();
            });
            ui.label(RichText::new("ai_lane_group").size(11.5));
            let mut group = c.ai_lane_group.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut group).desired_width(ui.available_width()))
                .lost_focus()
                && group != c.ai_lane_group
                && !group.is_empty()
            {
                c.ai_lane_group = group;
                cchanged = true;
            }
            let lane_count = app.scene.borrow().lanes.iter().filter(|l| l.container == ci).count();
            egui_kit::dim_text(ui, &format!("{lane_count} RoadLanes generated for this container"));
            if egui_kit::btn(ui, "Make RoadLanes Editable", false)
                .on_hover_text("Promote the generated lanes to editable RoadLane nodes")
                .clicked()
            {
                app.make_lanes_editable();
            }
        });

    egui::CollapsingHeader::new(RichText::new("Decoration").size(12.0).color(t.text_strong))
        .id_source("cont_deco")
        .default_open(false)
        .show(ui, |ui| {
            if ui
                .checkbox(&mut c.create_edge_curves, "create_edge_curves")
                .on_hover_text("Create edge curves (left / center / right Path3Ds)")
                .changed()
            {
                cchanged = true;
            }
        });

    ui.add_space(2.0);
    if egui_kit::btn(ui, "Export to gLTF", false)
        .on_hover_text("Export this container's generated geometry to dist/exports/<name>_geo.glb")
        .clicked()
    {
        let r = app.export_container_gltf(ci);
        show_action_result(app, r);
    }

    if cchanged {
        let mut sc = app.scene.borrow_mut();
        if ci < sc.curves.len() {
            sc.curves[ci] = c;
            sc.dirty = true;
            sc.recompute_junctions();
        }
        drop(sc);
        app.render.dirty = true;
    }

    divider(ui);
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Apply BRT median to all points", false).clicked() {
            let mut sc = app.scene.borrow_mut();
            for p in sc.curves[ci].points.iter_mut() {
                p.median = 3.2;
                p.pathway_l = 2.5;
                p.pathway_r = 2.5;
                p.lanes = p.lanes.max(4);
            }
            sc.dirty = true;
            sc.log("[edit] container-wide BRT Mwendokasi cross-section".to_string());
            drop(sc);
            app.render.dirty = true;
        }
    });
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Make dirt road", false).clicked() {
            let mut sc = app.scene.borrow_mut();
            for p in sc.curves[ci].points.iter_mut() {
                p.dirt = !p.dirt;
            }
            sc.dirty = true;
            drop(sc);
            app.render.dirty = true;
        }
        if egui_kit::btn(ui, "Delete container", false).clicked() {
            app.delete_container(ci);
        }
    });
}

/* ---- RoadIntersection inspector ---------------------------------- */

fn junction_inspector(app: &mut RoadkitApp, ui: &mut egui::Ui, ji: usize) {
    let info = {
        let sc = app.scene.borrow();
        let Some(j) = sc.junctions.get(ji) else {
            drop(sc);
            app.sel_junction = None;
            return;
        };
        (
            j.kind,
            j.legs
                .iter()
                .map(|l| (l.curve, l.is_end, sc.curves.get(l.curve).map(|c| c.name.clone()).unwrap_or_default()))
                .collect::<Vec<_>>(),
            j.x,
            j.z,
            j.prefab.as_ref().map(|p| p.id.clone()),
        )
    };
    let (kind, legs, x, z, current_prefab) = info;

    header(ui, &format!("RoadIntersection — {}", kind.label()));
    egui_kit::dim_text(ui, &format!("@ ({x:.0}, {z:.0}) · {} legs", legs.len()));
    egui_kit::dim_text(
        ui,
        &match &current_prefab {
            Some(id) => format!("WFC tile: {id}"),
            None => "procedural patch (no tile fit)".to_string(),
        },
    );

    let mode_idx = match kind {
        roads::JunctionKind::Crossing => 1,
        roads::JunctionKind::Roundabout => 2,
    };
    ui.label(RichText::new("mode").size(11.5));
    egui::ComboBox::from_id_source("junction_mode")
        .selected_text(["auto", "crossing", "roundabout"][mode_idx])
        .width(ui.available_width() - 8.0)
        .show_ui(ui, |ui| {
            let selectable = [
                (0usize, "auto (detect)", None),
                (1, "crossing (force)", Some(roads::JunctionKind::Crossing)),
                (2, "roundabout (force)", Some(roads::JunctionKind::Roundabout)),
            ];
            for (idx, label, force) in selectable {
                if ui.selectable_label(mode_idx == idx, label).clicked() {
                    let mut sc = app.scene.borrow_mut();
                    let key = ((sc.junctions[ji].x.round() as i64), (sc.junctions[ji].z.round() as i64));
                    match force {
                        Some(k) => {
                            sc.junction_override.insert(key, k);
                        }
                        None => {
                            sc.junction_override.remove(&key);
                        }
                    }
                    sc.recompute_junctions();
                    sc.log(format!("[junctions] #{ji} mode → {label}"));
                    drop(sc);
                    app.render.dirty = true;
                }
            }
        });

    let mut island_r = app.scene.borrow().junction_settings.island_radius;
    if egui_kit::drag_value_labeled_f64(ui, "island_radius", &mut island_r, 0.2).changed() {
        app.scene.borrow_mut().junction_settings.island_radius = island_r.clamp(2.0, 20.0);
        app.scene.borrow_mut().recompute_junctions();
        app.render.dirty = true;
    }

    // WFC prefab tile override (GRG custom-container GLBs)
    let key = ((x.round() as i64), (z.round() as i64));
    let current = app.scene.borrow().prefab_override.get(&key).cloned();
    ui.label(RichText::new("prefab tile (WFC)").size(11.5));
    let shown = current.clone().unwrap_or_else(|| "auto (WFC)".into());
    egui::ComboBox::from_id_source("junction_prefab")
        .selected_text(shown)
        .width(ui.available_width() - 8.0)
        .show_ui(ui, |ui| {
            let selectable: Vec<(String, String)> = {
                let mut v = vec![
                    ("auto (WFC)".to_string(), "auto (WFC)".to_string()),
                    ("procedural".to_string(), "procedural n-gon patch".to_string()),
                ];
                for d in roads::prefab::PREFABS {
                    v.push((d.id.to_string(), format!("{} · {}", d.label, d.id)));
                }
                v
            };
            for (value, label) in selectable {
                let sel = current.as_deref().unwrap_or("auto (WFC)") == value;
                if ui.selectable_label(sel, &label).clicked() {
                    let mut sc = app.scene.borrow_mut();
                    if value == "auto (WFC)" {
                        sc.prefab_override.remove(&key);
                    } else {
                        sc.prefab_override.insert(key, value.clone());
                    }
                    sc.recompute_junctions();
                    sc.log(format!("[prefabs] junction @ ({x:.0},{z:.0}) tile → {value}"));
                    drop(sc);
                    app.render.dirty = true;
                }
            }
        });

    divider(ui);
    header(ui, "Legs");
    for (_curve, is_end, name) in &legs {
        egui_kit::dim_text(ui, &format!("└ {} '{name}' {}", if *is_end { "end" } else { "through" }, if *is_end { "(RoadPoint)" } else { "(crossing leg)" }));
    }
    if let Some(def) = app.schema.class("RoadIntersection") {
        divider(ui);
        egui_kit::dim_text(ui, &def.description);
    }
}

/* ---- RoadManager inspector (nothing selected) --------------------- */

fn scene_inspector(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    header(ui, "RoadManager");
    egui_kit::dim_text(ui, "No selection — pick a row in Layers or click the plan (V).");
    ui.add_space(2.0);

    // the manager node owns defaults for every container
    let mut mg = app.scene.borrow().manager.clone();
    let mut mg_changed = false;

    egui::CollapsingHeader::new(RichText::new("Road Generation").size(12.0).color(t.text_strong))
        .id_source("mgr_roadgen")
        .default_open(true)
        .show(ui, |ui| {
            egui::Grid::new("mgr_gen_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
                ui.label(RichText::new("density").size(11.5));
                if ui
                    .add(egui::DragValue::new(&mut mg.density).speed(0.25).suffix(" m"))
                    .on_hover_text("Meters between road loop cuts (GRG RoadSegment.DEFAULT_DENSITY = 4)")
                    .changed()
                {
                    mg_changed = true;
                }
                ui.end_row();
                ui.label(RichText::new("underside_thickness").size(11.5));
                if ui
                    .add(egui::DragValue::new(&mut mg.underside_thickness).speed(0.1).suffix(" m"))
                    .on_hover_text("-1 = no underside generated")
                    .changed()
                {
                    mg_changed = true;
                }
                ui.end_row();
            });
        });

    egui::CollapsingHeader::new(RichText::new("Lanes and AI").size(12.0).color(t.text_strong))
        .id_source("mgr_lanes")
        .default_open(true)
        .show(ui, |ui| {
            ui.label(RichText::new("ai_lane_group").size(11.5));
            let mut group = mg.ai_lane_group.clone();
            if ui
                .add(egui::TextEdit::singleline(&mut group).desired_width(ui.available_width()))
                .lost_focus()
                && group != mg.ai_lane_group
                && !group.is_empty()
            {
                mg.ai_lane_group = group;
                mg_changed = true;
            }
            let n = app.scene.borrow().lanes.len();
            egui_kit::dim_text(ui, &format!("{n} RoadLanes in the scene"));
        });

    egui::CollapsingHeader::new(RichText::new("Editor settings").size(12.0).color(t.text_strong))
        .id_source("mgr_editor")
        .default_open(false)
        .show(ui, |ui| {
            if ui
                .checkbox(&mut mg.auto_refresh, "auto_refresh")
                .on_hover_text("Regenerate junctions + lanes automatically on transforms")
                .changed()
            {
                mg_changed = true;
            }
        });

    if mg_changed {
        app.scene.borrow_mut().manager = mg;
        app.scene.borrow_mut().dirty = true;
        app.render.dirty = true;
    }

    /* ---- RoadTerrain3DConnector ---- */
    divider(ui);
    header(ui, "RoadTerrain3DConnector");
    let terrain_on = app.scene.borrow().terrain.enabled;
    if !terrain_on {
        if egui_kit::btn(ui, "Add RoadTerrain3DConnector", false)
            .on_hover_text("Add the terrain connector — rolling hills, flattened under roads")
            .clicked()
        {
            app.add_terrain_connector();
        }
    } else {
        let mut ter = app.scene.borrow().terrain.clone();
        let mut on = true;
        if ui.checkbox(&mut on, "Set terrain").on_hover_text("Toggle the connector (history-named like Godot)").changed() {
            app.set_terrain_enabled(on);
        }
        let mut ter_changed = false;
        egui::Grid::new("terrain_grid").num_columns(2).spacing([6.0, 4.0]).show(ui, |ui| {
            ui.label(RichText::new("amplitude").size(11.5));
            if ui.add(egui::DragValue::new(&mut ter.amplitude).speed(0.25).suffix(" m").range(0.0..=60.0)).changed() {
                ter_changed = true;
            }
            ui.end_row();
            ui.label(RichText::new("wavelength").size(11.5));
            if ui.add(egui::DragValue::new(&mut ter.wavelength).speed(1.0).suffix(" m").range(10.0..=2000.0)).changed() {
                ter_changed = true;
            }
            ui.end_row();
            ui.label(RichText::new("offset").size(11.5));
            if ui.add(egui::DragValue::new(&mut ter.offset).speed(0.05).suffix(" m").range(-5.0..=5.0)).changed() {
                ter_changed = true;
            }
            ui.end_row();
            ui.label(RichText::new("edge_margin").size(11.5));
            if ui.add(egui::DragValue::new(&mut ter.edge_margin).speed(0.05).suffix(" m").range(0.0..=10.0)).changed() {
                ter_changed = true;
            }
            ui.end_row();
            ui.label(RichText::new("edge_falloff").size(11.5));
            if ui.add(egui::DragValue::new(&mut ter.edge_falloff).speed(0.1).suffix(" m").range(0.5..=40.0)).changed() {
                ter_changed = true;
            }
            ui.end_row();
            ui.label(RichText::new("flatten_under_roads").size(11.5));
            if ui.checkbox(&mut ter.flatten_under_roads, "").changed() {
                ter_changed = true;
            }
            ui.end_row();
        });
        if ter_changed {
            app.scene.borrow_mut().terrain = ter;
            app.render.dirty = true;
        }
    }

    divider(ui);
    header(ui, "Junction engine");
    let mut js = app.scene.borrow().junction_settings.clone();
    let mut changed = false;
    if egui_kit::checkbox_row(ui, "auto-join endpoints + crossings", &mut js.enabled).changed() {
        changed = true;
    }
    if egui_kit::checkbox_row(ui, "3+ road meetings become roundabouts", &mut js.auto_roundabout).changed() {
        changed = true;
    }
    if egui_kit::drag_value_labeled_f64(ui, "snap threshold", &mut js.snap_m, 0.5).changed() {
        changed = true;
    }
    if egui_kit::drag_value_labeled_f64(ui, "island radius", &mut js.island_radius, 0.2).changed() {
        changed = true;
    }
    if changed {
        app.scene.borrow_mut().junction_settings = js;
        app.scene.borrow_mut().recompute_junctions();
        app.render.dirty = true;
    }
    divider(ui);
    header(ui, "Assets");
    egui_kit::dim_text(ui, &format!("{} GLB assets · {} textures loaded", app.assets.entries.len(), app.textures.slot_count()));
}

/* ================================================================== */
/* ASSETS                                                              */
/* ================================================================== */

fn assets_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    egui_kit::dim_text(ui, &format!("{} GLB prop assets — roads stay procedural (GRG-style)", app.assets.entries.len()));
    ui.add_space(3.0);

    for i in 0..app.assets.entries.len() {
        let (id, name, role, tris) = {
            let e = &app.assets.entries[i];
            (e.id.clone(), e.name.clone(), e.role.clone(), e.mesh.triangle_count())
        };
        let count = app.scene.borrow().props.iter().filter(|p| p.asset == id).count();
        ui.horizontal(|ui| {
            let thumb = app.assets.entries[i].thumb.clone();
            if let Some(thumb) = thumb {
                let img = egui::ColorImage::from_rgba_unmultiplied([96, 96], &thumb);
                let tex = ui.ctx().load_texture(format!("thumb-{id}"), img, egui::TextureOptions::LINEAR);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 4.0, t.input);
                ui.image((tex.id(), egui::vec2(38.0, 38.0)));
            }
            ui.vertical(|ui| {
                ui.label(RichText::new(&name).strong().size(11.5).color(t.text));
                egui_kit::dim_text(ui, &format!("{role} · {tris} tris · {count} placed"));
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let selected = app.stamp_asset == id;
                if ui
                    .add(
                        egui::Button::new(if selected { RichText::new("●").color(t.accent_strong) } else { RichText::new("Stamp").color(t.text) })
                            .fill(if selected { t.tool_active } else { t.button })
                            .min_size(egui::vec2(52.0, 20.0)),
                    )
                    .on_hover_text("Select, then click a road in the 2D plan")
                    .clicked()
                {
                    app.stamp_asset = id.clone();
                    app.tool = Tool::Stamp;
                }
            });
        });
        ui.add_space(2.0);
    }

    divider(ui);
    ui.horizontal(|ui| {
        ui.label(RichText::new("stamp lateral").size(11.0).color(t.text_dim));
        ui.add(egui::DragValue::new(&mut app.stamp_lateral).speed(0.25).suffix(" m").range(-20.0..=20.0));
    });
    if egui_kit::btn(ui, "▶ Join GLB props like Godot", true).clicked() {
        app.join_props_like_godot();
    }
    if egui_kit::btn(ui, "Clear all props", false).clicked() {
        app.clear_props();
    }
}

/* ================================================================== */
/* SCRIPT                                                              */
/* ================================================================== */

fn script_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    ui.horizontal(|ui| {
        let labels: Vec<&str> = crate::scripts::all().iter().map(|(l, _, _)| *l).collect();
        egui::ComboBox::from_id_source("script_picker")
            .selected_text(labels.get(app.script_idx).copied().unwrap_or("custom"))
            .width(ui.available_width() - 74.0)
            .show_ui(ui, |ui| {
                for (i, (label, file, _)) in crate::scripts::all().into_iter().enumerate() {
                    if ui.selectable_label(app.script_idx == i, format!("{label}  ({file})")).clicked() {
                        app.set_script(i);
                    }
                }
            });
        if ui
            .add(
                egui::Button::new(RichText::new("▶").strong().color(t.text_strong))
                    .fill(t.accent_strong)
                    .min_size(egui::vec2(30.0, 24.0)),
            )
            .on_hover_text("Parse + run the .gd script (F5)")
            .clicked()
        {
            app.run_script();
        }
    });
    ui.add_space(3.0);

    let mut src = app.script_src.clone();
    let before = src.clone();
    ScrollArea::vertical()
        .id_source("script_src")
        .max_height(300.0)
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut src)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(14),
            );
        });
    if src != before {
        app.script_src = src;
    }

    match &app.script_status {
        ScriptStatus::Idle => egui_kit::dim_text(ui, "idle — the script runs on startup and with F5"),
        ScriptStatus::Ok(m) => {
            ui.label(RichText::new(format!("✓ {m}")).size(10.5).color(Color32::from_rgb(0x7d, 0xe2, 0x8a)));
        }
        ScriptStatus::Err(m) => {
            ui.label(RichText::new(format!("✗ {m}")).size(10.5).color(Color32::from_rgb(0xf2, 0x7b, 0x6d)));
        }
    }
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Reset scene", false).clicked() {
            app.reset_scene();
        }
        if egui_kit::btn(ui, "API reference", false).clicked() {
            app.show_api = true;
        }
    });
}

/* ================================================================== */
/* CONSOLE                                                             */
/* ================================================================== */

fn console_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    let lines = app.scene.borrow().console.clone();
    if lines.is_empty() {
        egui_kit::dim_text(ui, "— no output —");
    }
    for l in &lines {
        let color = if l.starts_with("[error]") {
            Color32::from_rgb(0xf2, 0x7b, 0x6d)
        } else if l.starts_with("[props]") {
            Color32::from_rgb(0xe8, 0xc4, 0x7d)
        } else if l.starts_with("[road]") || l.starts_with("[layers]") {
            Color32::from_rgb(0x9d, 0xd3, 0xa0)
        } else if l.starts_with('[') {
            t.text_dim
        } else {
            t.text
        };
        ui.label(RichText::new(l).monospace().size(10.5).color(color));
    }
    app.console_seen = lines.len();
}

/* ================================================================== */
/* HISTORY — the Godot editor History dock (undo/redo timeline)        */
/* ================================================================== */

fn history_content(app: &mut RoadkitApp, ui: &mut egui::Ui) {
    let t = tokens(ui.ctx());
    // header row: Undo / Redo + snapshot toggles
    ui.horizontal(|ui| {
        if egui_kit::btn(ui, "Undo", false).on_hover_text("Step back one action").clicked() {
            let _ = actions::perform(app, "history_undo");
        }
        if egui_kit::btn(ui, "Redo", false).on_hover_text("Step forward one action").clicked() {
            let _ = actions::perform(app, "history_redo");
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if egui_kit::icon_button_flat(ui, IconName::Trash, false, "Clear the undo history").clicked() {
                let _ = actions::perform(app, "history_clear");
            }
        });
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut app.history_scene, "Scene")
            .on_hover_text("Snapshot the scene graph with every action");
        ui.checkbox(&mut app.history_global, "Global")
            .on_hover_text("Snapshot the camera + selection with every action");
    });
    divider(ui);

    // the entries, newest at TOP like Godot's editor; the live cursor row is
    // accent-highlighted, rows above it (already undone) are dimmed
    let cursor = app.history.cursor;
    let entries: Vec<(usize, String)> = app
        .history
        .entries
        .iter()
        .enumerate()
        .map(|(i, e)| (i, e.label.clone()))
        .collect();
    if entries.is_empty() {
        egui_kit::dim_text(ui, "— empty —");
    }
    for (i, label) in entries.iter().rev() {
        let is_current = *i == cursor;
        let undone = *i > cursor;
        let row_h = 20.0;
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
        if is_current {
            ui.painter().rect_filled(rect, 4.0, t.tool_active);
            // the accent tick, like Godot's blue current-step highlight
            ui.painter().rect_filled(
                egui::Rect::from_min_size(egui::pos2(rect.left() + 1.0, rect.top() + 3.0), egui::vec2(3.0, row_h - 6.0)),
                2.0,
                t.accent,
            );
        } else if resp.hovered() {
            ui.painter().rect_filled(rect, 4.0, t.hover.gamma_multiply(0.5));
        }
        ui.painter().text(
            egui::pos2(rect.left() + 11.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(11.0),
            if is_current {
                t.text_strong
            } else if undone {
                t.text_disabled
            } else {
                t.text
            },
        );
        if resp.clicked() {
            app.restore_history(*i);
        }
        resp.on_hover_text(if is_current {
            "current state"
        } else if undone {
            "click to redo up to this step"
        } else {
            "click to undo back to this step"
        });
    }
    divider(ui);
    egui_kit::dim_text(ui, &format!(
        "cursor {}/{}",
        app.history.cursor,
        app.history.entries.len().saturating_sub(1)
    ));
    egui_kit::dim_text(ui, "click a row to jump — dimmed rows are the redo tail");
}

/* ================================================================== */
/* Dialogs                                                             */
/* ================================================================== */

pub fn dialogs(app: &mut RoadkitApp, ctx: &egui::Context) {
    if app.show_api {
        let mut open = true;
        egui::Window::new(".gd scripting API")
            .open(&mut open)
            .resizable(true)
            .default_width(520.0)
            .show(ctx, |ui| {
                ScrollArea::vertical().max_height(480.0).show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut crate::scripts::api_text().to_string())
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(30)
                            .interactive(true),
                    );
                });
            });
        if !open {
            app.show_api = false;
        }
    }
    if app.show_about {
        let mut open = true;
        egui::Window::new("About")
            .open(&mut open)
            .resizable(false)
            .default_width(440.0)
            .show(ctx, |ui| {
                ui.label(RichText::new("RoadKit Godot 2.3").strong().size(15.0));
                ui.label("Pure-Rust road studio in the spirit of godot-road-generator:");
                ui.label("• roads are PROCEDURAL — RoadPoint magnitudes drive cubic Beziers, swept into textured lane/shoulder meshes");
                ui.label("• 28 GLB assets — props, junction prefabs, TZ/US/EU traffic lights, road signs, BRT Mwendokasi median stations");
                ui.label("• layer engine: RoadManager → RoadContainer → RoadPoint with eye/lock, synced to plan + properties");
                ui.label("• auto junctions: endpoint welds, n-gon crossings, T-junctions, roundabouts with islands, WFC prefab tiles");
                ui.label("• a built-in .gd parser + interpreter runs Godot-style scripts that drive the whole scene");
                ui.label("• dependency-free parallel software 3D renderer (z-buffered, lambert, perspective-correct textures)");
                ui.label("• MCP testing server: run with --mcp, or --uitest to exercise every UI action headlessly");
                ui.add_space(4.0);
                ui.label(RichText::new("crates: gdmath · gdscript · roads · roadkit · egui-kit · egui-app · xtask").monospace().size(11.0));
            });
        if !open {
            app.show_about = false;
        }
    }
}
