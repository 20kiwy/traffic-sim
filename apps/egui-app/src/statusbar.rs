//! Bottom chrome — status bar (24 px) + hint bar (28 px), VectorCraft style.

use crate::state::RoadkitApp;
use egui::RichText;

pub const STATUS_H: f32 = 24.0;
pub const HINT_H: f32 = 28.0;

pub fn status_bar(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::TopBottomPanel::bottom("status_bar")
        .exact_height(STATUS_H)
        .frame(
            egui::Frame::none()
                .fill(t.panel)
                .stroke(egui::Stroke::new(1.0, t.border))
                .inner_margin(egui::Margin::symmetric(8.0, 0.0)),
        )
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let sc = app.scene.borrow();
                let (curves, points, props, junctions) = (
                    sc.curves.len(),
                    sc.curves.iter().map(|c| c.points.len()).sum::<usize>(),
                    sc.props.len(),
                    sc.junctions.len(),
                );
                let tris = app.render.scene3d.as_ref().map(|s| s.stats.tris).unwrap_or(0);
                drop(sc);

                ui.label(RichText::new(app.tool.label()).strong().size(11.0).color(t.text_strong));
                ui.separator();
                ui.label(
                    RichText::new(format!(
                        "roads {curves} · points {points} · props {props} · junctions {junctions} · tris {tris}"
                    ))
                    .size(11.0)
                    .color(t.text_dim),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(RichText::new(format!("{:.0} fps", app.fps)).size(11.0).color(t.text_dim));
                    if !app.status.is_empty() {
                        ui.label(RichText::new(&app.status).size(11.0).color(t.text));
                    }
                });
            });
        });
}

pub fn hint_bar(app: &mut RoadkitApp, ctx: &egui::Context) {
    let t = egui_kit::tokens(ctx);
    egui::TopBottomPanel::bottom("hint_bar")
        .exact_height(HINT_H)
        .frame(egui::Frame::none().fill(t.panel_darker).inner_margin(egui::Margin::symmetric(10.0, 0.0)))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let hint = match app.tool {
                    crate::state::Tool::Select => "click a RoadPoint to select · drag to move (endpoints auto-join) · Layers panel syncs the selection",
                    crate::state::Tool::AddPoint => "click the 2D plan to append a RoadPoint to the selected road — new points snap to nearby endpoints",
                    crate::state::Tool::Stamp => "click a road in the 2D plan to stamp the selected GLB asset (lateral offset in Assets)",
                    crate::state::Tool::Pan => "drag to pan · scroll to zoom · F fits the scene",
                };
                ui.label(RichText::new(hint).size(11.0).color(t.text_dim));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new("F5 run · F fit · V select · A add · P stamp · H pan")
                            .size(10.5)
                            .color(t.text_disabled),
                    );
                });
            });
        });
}
