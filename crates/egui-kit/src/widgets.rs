//! Shared panel widgets — the VectorCraft chrome patterns: tab strips, layer-tree
//! rows with eye/lock columns, collapsible sections, search fields, numeric drags.

use crate::icons::IconName;
use crate::theme::tokens;
use egui::{Align2, Color32, Margin, Pos2, Rect, Rounding, Sense, Stroke, Ui, Vec2};

/// A tab strip like the dock's `Properties | Layers | Libraries | Tiles` header.
/// Returns the index of the active tab (click-activated).
pub fn tab_bar(ui: &mut Ui, tabs: &[&str], active: usize) -> Option<usize> {
    let t = tokens(ui.ctx());
    let mut clicked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        for (i, label) in tabs.iter().enumerate() {
            let selected = i == active;
            let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width() / tabs.len().max(1) as f32, 26.0), Sense::click());
            if selected {
                ui.painter().rect_filled(rect, 0.0, t.panel_tinted(0.10));
                ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(2.0, t.accent));
            } else if resp.hovered() {
                ui.painter().rect_filled(rect, 0.0, t.hover);
            }
            ui.painter().line_segment([rect.right_top(), rect.right_bottom()], Stroke::new(1.0, t.border));
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                *label,
                egui::FontId::proportional(if selected { 12.5 } else { 12.0 }),
                if selected { t.text } else { t.text_dim },
            );
            if resp.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// A tree row: disclosure arrow, icon, name, and optional trailing widgets.
/// `depth` drives the indent. Returns the row response (clicked = select).
pub struct TreeRowStyle {
    pub row_h: f32,
    pub selected: bool,
    pub dim: bool,
    pub colour_bar: Option<Color32>,
    pub bar_w: f32,
    pub icon: Option<IconName>,
    pub icon_size: f32,
}

impl Default for TreeRowStyle {
    fn default() -> Self {
        Self { row_h: 24.0, selected: false, dim: false, colour_bar: None, bar_w: 3.0, icon: None, icon_size: 14.0 }
    }
}

pub fn tree_row(ui: &mut Ui, depth: usize, open: Option<bool>, name: &str, style: &TreeRowStyle, trailing: impl FnOnce(&mut Ui, &Rect)) -> egui::Response {
    let t = tokens(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), style.row_h), Sense::click());
    if style.selected {
        ui.painter().rect_filled(rect, 0.0, t.row_selected);
        ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.accent.gamma_multiply(0.6)));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, t.hover);
    }
    let mut x = rect.left() + 4.0 + depth as f32 * 14.0;
    // disclosure
    if let Some(open) = open {
        let ic = Rect::from_center_size(Pos2::new(x + 6.0, rect.center().y), Vec2::splat(10.0));
        crate::icons::paint(ui, if open { IconName::ChevronDown } else { IconName::ChevronRight }, ic, t.text_dim);
    }
    x += if open.is_some() { 14.0 } else { 2.0 };
    // colour bar (the layer stripe)
    if let Some(col) = style.colour_bar {
        ui.painter().rect_filled(Rect::from_min_size(Pos2::new(x, rect.top() + 3.0), Vec2::new(style.bar_w, style.row_h - 6.0)), 1.0, col);
        x += style.bar_w + 5.0;
    }
    // icon
    if let Some(icn) = style.icon {
        let ir = Rect::from_center_size(Pos2::new(x + style.icon_size / 2.0, rect.center().y), Vec2::splat(style.icon_size));
        let tint = if style.dim { t.text_disabled } else { t.icon };
        crate::icons::paint(ui, icn, ir, tint);
        x += style.icon_size + 5.0;
    }
    // name
    let tint = if style.dim { t.text_disabled } else if style.selected { t.text_strong } else { t.text };
    ui.painter().text(Pos2::new(x, rect.center().y), Align2::LEFT_CENTER, name, egui::FontId::proportional(t.font_size), tint);
    let mut tr = rect;
    tr.min.x = rect.right() - 70.0;
    trailing(ui, &tr);
    resp
}

/// The eye + lock columns of a tree row (VectorCraft `eye_and_lock`): two 16 pt
/// columns with hairline separators, drawn at the right edge of the row.
pub fn eye_lock_columns(ui: &mut Ui, rect: &Rect, visible: bool, locked: bool) -> (bool, bool) {
    let t = tokens(ui.ctx());
    let h = rect.height();
    let eye = Rect::from_min_size(egui::pos2(rect.right() - 34.0, rect.top()), Vec2::new(17.0, h));
    let lock = Rect::from_min_size(egui::pos2(rect.right() - 17.0, rect.top()), Vec2::new(17.0, h));
    ui.painter().line_segment([eye.right_top(), eye.right_bottom()], Stroke::new(1.0, t.input_border));
    ui.painter().line_segment([lock.right_top(), lock.right_bottom()], Stroke::new(1.0, t.input_border));
    let er = ui.interact(eye, ui.id().with(("eye", (rect.min.x as i32, rect.min.y as i32))), Sense::click());
    let lr = ui.interact(lock, ui.id().with(("lock", (rect.min.x as i32, rect.min.y as i32))), Sense::click());
    if visible {
        crate::icons::paint(ui, IconName::Eye, Rect::from_center_size(eye.center(), Vec2::splat(12.0)), t.icon);
    } else {
        crate::icons::paint(ui, IconName::EyeOff, Rect::from_center_size(eye.center(), Vec2::splat(12.0)), t.text_disabled);
    }
    if locked {
        crate::icons::paint(ui, IconName::Lock, Rect::from_center_size(lock.center(), Vec2::splat(11.0)), t.icon);
    } else if lr.hovered() {
        crate::icons::paint(ui, IconName::Unlock, Rect::from_center_size(lock.center(), Vec2::splat(11.0)), t.text_disabled);
    }
    (er.clicked(), lr.clicked())
}

/// The dock header with the » / « collapse arrow (VectorCraft `collapse_header`).
/// True when clicked.
pub fn collapse_header(ui: &mut Ui, collapsed: bool) -> bool {
    let t = tokens(ui.ctx());
    let (hdr, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 14.0), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(hdr, 0.0, t.hover);
    }
    let arrow = Rect::from_min_size(hdr.right_top() + egui::vec2(-16.0, 1.0), Vec2::new(12.0, 12.0));
    crate::icons::paint(ui, if collapsed { IconName::ChevronsLeft } else { IconName::ChevronsRight }, arrow, if resp.hovered() { t.text } else { t.text_dim });
    resp.clicked()
}

/// A collapsible section header (Inspector groups). Toggled by clicking; the open
/// state is stored in egui memory under `id`.
pub fn section(ui: &mut Ui, id: &str, label: &str) -> bool {
    let t = tokens(ui.ctx());
    let open_id = egui::Id::new(("section", id));
    let mut open = ui.ctx().data(|d| d.get_temp::<bool>(open_id).unwrap_or(true));
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click());
    ui.painter().rect_filled(rect, 0.0, t.panel_darker);
    let ic = Rect::from_center_size(Pos2::new(rect.left() + 9.0, rect.center().y), Vec2::splat(10.0));
    crate::icons::paint(ui, if open { IconName::ChevronDown } else { IconName::ChevronRight }, ic, t.text_dim);
    ui.painter().text(Pos2::new(rect.left() + 20.0, rect.center().y), Align2::LEFT_CENTER, label.to_uppercase(), egui::FontId::proportional(10.5), t.text_dim);
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, t.border));
    if resp.clicked() {
        open = !open;
        ui.ctx().data_mut(|d| d.insert_temp(open_id, open));
    }
    open
}

/// The `Search All` dark rounded field.
pub fn search_field(ui: &mut Ui, id: &str, buf: &mut String) -> egui::Response {
    let t = tokens(ui.ctx());
    let frame = egui::Frame::none().fill(t.input).stroke(Stroke::new(1.0, t.input_border)).rounding(Rounding::same(4.0)).inner_margin(Margin::symmetric(8.0, 5.0));
    let inner = egui::TextEdit::singleline(buf).id_source(id).hint_text("Search All").font(egui::TextStyle::Body).frame(false).desired_width(f32::INFINITY);
    frame.show(ui, |ui| ui.add(inner)).response
}

/// Label + numeric drag row (Inspector field): returns the new value.
pub fn drag_value_labeled(ui: &mut Ui, label: &str, value: &mut f32, speed: f32) -> egui::Response {
    let t = tokens(ui.ctx());
    ui.horizontal(|ui| {
        let lw = 92.0;
        let (lr, _) = ui.allocate_exact_size(Vec2::new(lw, 18.0), Sense::hover());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, label, egui::FontId::proportional(11.5), t.text_dim);
        let fr = egui::Frame::none().fill(t.input).stroke(Stroke::new(1.0, t.input_border)).rounding(Rounding::same(3.0)).inner_margin(Margin::symmetric(6.0, 2.0));
        fr.show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::DragValue::new(value).speed(speed));
            });
        })
        .response
    })
    .inner
}

/// f64 flavour for the metric fields (meters).
pub fn drag_value_labeled_f64(ui: &mut Ui, label: &str, value: &mut f64, speed: f64) -> egui::Response {
    let t = tokens(ui.ctx());
    ui.horizontal(|ui| {
        let lw = 92.0;
        let (lr, _) = ui.allocate_exact_size(Vec2::new(lw, 18.0), Sense::hover());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, label, egui::FontId::proportional(11.5), t.text_dim);
        let fr = egui::Frame::none().fill(t.input).stroke(Stroke::new(1.0, t.input_border)).rounding(Rounding::same(3.0)).inner_margin(Margin::symmetric(6.0, 2.0));
        fr.show(ui, |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add(egui::DragValue::new(value).speed(speed).suffix(" m"));
            });
        })
        .response
    })
    .inner
}

/// Label + combo row.
pub fn combo(ui: &mut Ui, label: &str, chosen: &mut String, options: &[String]) -> bool {
    let t = tokens(ui.ctx());
    let mut changed = false;
    ui.horizontal(|ui| {
        let (lr, _) = ui.allocate_exact_size(Vec2::new(92.0, 18.0), Sense::hover());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, label, egui::FontId::proportional(11.5), t.text_dim);
        egui::ComboBox::from_id_source(label).width(ui.available_width() - 8.0).selected_text(chosen.clone()).show_ui(ui, |ui| {
            for o in options {
                if ui.selectable_label(*chosen == *o, o).clicked() && *chosen != *o {
                    *chosen = o.clone();
                    changed = true;
                }
            }
        });
    });
    changed
}

/// Small square icon button with an active state (tool strips, panel headers).
pub fn icon_button(ui: &mut Ui, name: IconName, active: bool, tip: &str) -> egui::Response {
    let t = tokens(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(28.0, 24.0), Sense::click());
    let well = Rect::from_center_size(rect.center(), Vec2::new(24.0, 21.0));
    if active {
        ui.painter().rect_filled(well, Rounding::same(5.0), t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(well, Rounding::same(5.0), t.hover);
    }
    crate::icons::paint(ui, name, Rect::from_center_size(rect.center(), Vec2::splat(15.0)), if active { t.text_strong } else { t.icon });
    resp.on_hover_text(tip)
}

/// A flat icon button for bars (no well, hover tint only) — control-bar style.
pub fn icon_button_flat(ui: &mut Ui, name: IconName, active: bool, tip: &str) -> egui::Response {
    let t = tokens(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(24.0, 22.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, Rounding::same(4.0), t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, Rounding::same(4.0), t.hover);
    }
    crate::icons::paint(ui, name, Rect::from_center_size(rect.center(), Vec2::splat(14.0)), if active { t.text_strong } else if resp.hovered() { t.text } else { t.icon });
    resp.on_hover_text(tip)
}

/// Tool-rail button: icon well on top + a small text caption underneath, so
/// every control is readable (not icon-only).
pub fn icon_button_caption(ui: &mut Ui, name: IconName, active: bool, caption: &str, tip: &str) -> egui::Response {
    let t = tokens(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let well = Rect::from_center_size(Pos2::new(rect.center().x, rect.top() + 13.0), Vec2::new(26.0, 22.0));
    if active {
        ui.painter().rect_filled(well, Rounding::same(5.0), t.tool_active);
    } else if resp.hovered() {
        ui.painter().rect_filled(well, Rounding::same(5.0), t.hover);
    }
    crate::icons::paint(
        ui,
        name,
        Rect::from_center_size(well.center(), Vec2::splat(15.0)),
        if active { t.text_strong } else { t.icon },
    );
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 5.0),
        Align2::CENTER_CENTER,
        caption,
        egui::FontId::proportional(9.0),
        if active { t.text_strong } else { t.text_dim },
    );
    resp.on_hover_text(tip)
}

/// A text button (Properties panel actions).
pub fn btn(ui: &mut Ui, label: &str, accent: bool) -> egui::Response {
    let t = tokens(ui.ctx());
    let fill = if accent { t.accent } else { t.button };
    let text_c = if accent { t.text_strong } else { t.text };
    let resp = ui.add(egui::Button::new(egui::RichText::new(label).color(text_c).size(12.0)).fill(fill).stroke(Stroke::new(1.0, if accent { t.accent_strong } else { t.button_border })).rounding(Rounding::same(4.0)).min_size(Vec2::new(0.0, 24.0)));
    if !accent && resp.hovered() {
        // hover fill handled by egui itself
    }
    resp
}

/// A checkbox row with label.
pub fn checkbox_row(ui: &mut Ui, label: &str, on: &mut bool) -> egui::Response {
    ui.horizontal(|ui| {
        let r = ui.add(egui::Checkbox::new(on, ""));
        let t = tokens(ui.ctx());
        let (lr, lresp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), Sense::click());
        ui.painter().text(lr.left_center(), Align2::LEFT_CENTER, label, egui::FontId::proportional(12.0), t.text);
        if lresp.clicked() {
            *on = !*on;
        }
        r | lresp
    })
    .inner
}

/// A small colour swatch square.
pub fn color_swatch(ui: &mut Ui, color: Color32, size: f32) -> egui::Response {
    let t = tokens(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    ui.painter().rect_filled(rect, 2.0, color);
    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, t.input_border));
    resp
}

pub fn dim_text(ui: &mut Ui, s: &str) {
    let t = tokens(ui.ctx());
    ui.label(egui::RichText::new(s).size(11.5).color(t.text_dim));
}

/// A floating panel window with the card frame and a draggable title strip.
/// Returns None while open (egui windows manage their own close).
pub fn floating_window(ctx: &egui::Context, id: &str, title: &str, open: &mut bool, add: impl FnOnce(&mut Ui)) {
    let t = tokens(ctx);
    let mut open_local = *open;
    let mut win = egui::Window::new(egui::RichText::new(title).color(t.text).size(12.5))
        .id(egui::Id::new(id))
        .open(&mut open_local)
        .resizable(true)
        .collapsible(false)
        .default_width(300.0)
        .default_height(360.0)
        .frame(crate::theme::card_frame(ctx).inner_margin(Margin::same(0.0)))
        .title_bar(false);
    win.show(ctx, |ui| {
            // custom title strip
            let (hdr, hresp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::click_and_drag());
            ui.painter().rect_filled(hdr, 0.0, t.tab_strip);
            ui.painter().text(Pos2::new(hdr.left() + 10.0, hdr.center().y), Align2::LEFT_CENTER, title, egui::FontId::proportional(12.0), t.text);
            let close = Rect::from_min_size(hdr.right_top() + egui::vec2(-20.0, 3.0), Vec2::new(16.0, 16.0));
            let cr = ui.interact(close, egui::Id::new((id, "close")), Sense::click());
            crate::icons::paint(ui, IconName::Close, close.shrink(2.0), if cr.hovered() { t.text } else { t.text_dim });
            if cr.clicked() {
                *open = false;
            }
            if hresp.hovered() && hresp.dragged_by(egui::PointerButton::Primary) {
                // handled by the Window itself via drag bounds; nothing extra
            }
            ui.separator();
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, add);
        });
    *open = open_local;
}

pub fn row_selected_bg(ui: &mut Ui, rect: &Rect) {
    let t = tokens(ui.ctx());
    ui.painter().rect_filled(*rect, 0.0, t.row_selected);
}
