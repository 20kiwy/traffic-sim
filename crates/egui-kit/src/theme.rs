//! Design tokens (four UI brightness levels), fonts and egui style.
//!
//! Ported 1:1 from VectorCraft's `theme.rs` (the Craft look): cool slate greys,
//! flat faces with soft corners, and one accent for what is on, chosen or focused.

use egui::{Color32, Rounding, FontDefinitions, FontFamily, Margin, Stroke, TextStyle, Visuals};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Brightness {
    Dark,
    #[default]
    MediumDark,
    MediumLight,
    Light,
}

impl Brightness {
    pub const ALL: [Brightness; 4] = [Brightness::Dark, Brightness::MediumDark, Brightness::MediumLight, Brightness::Light];
    pub fn label(self) -> &'static str {
        match self {
            Brightness::Dark => "Dark",
            Brightness::MediumDark => "Medium Dark",
            Brightness::MediumLight => "Medium Light",
            Brightness::Light => "Light",
        }
    }
}

/// The one colour for what is on, chosen or focused. Coral is the default.
/// (R13: the Blue accent was removed — the app's highlight language is
/// warm coral/amber only.)
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Accent {
    #[default]
    Coral,
    Violet,
    Green,
    Amber,
    Pink,
}

impl Accent {
    pub const ALL: [Accent; 5] = [Accent::Coral, Accent::Violet, Accent::Green, Accent::Amber, Accent::Pink];
    pub fn label(self) -> &'static str {
        match self {
            Accent::Coral => "Coral",
            Accent::Violet => "Violet",
            Accent::Green => "Green",
            Accent::Amber => "Amber",
            Accent::Pink => "Pink",
        }
    }
    fn hex(self) -> u32 {
        match self {
            Accent::Coral => 0xf06a52,
            Accent::Violet => 0x9a6aff,
            Accent::Green => 0x3fae6a,
            Accent::Amber => 0xd99922,
            Accent::Pink => 0xd94f9a,
        }
    }
    fn strong_hex(self) -> u32 {
        match self {
            Accent::Coral => 0xe8573f,
            Accent::Violet => 0x8a58f0,
            Accent::Green => 0x369a5d,
            Accent::Amber => 0xc98a1a,
            Accent::Pink => 0xc74489,
        }
    }
    fn flyout_hex(self) -> u32 {
        match self {
            Accent::Coral => 0xff8a74,
            Accent::Violet => 0xb08aff,
            Accent::Green => 0x62c288,
            Accent::Amber => 0xe8b04a,
            Accent::Pink => 0xe478ae,
        }
    }
}

/// The visual vocabulary of the app, at one brightness. Field names follow
/// VectorCraft's `Tokens` so the mapping stays obvious.
#[derive(Clone, Copy, Debug)]
pub struct Tokens {
    pub dark: bool,
    pub app_bar: Color32,
    pub panel: Color32,
    pub panel_darker: Color32,
    pub tab_strip: Color32,
    pub input: Color32,
    pub input_border: Color32,
    pub border: Color32,
    pub divider: Color32,
    pub text: Color32,
    pub text_strong: Color32,
    pub text_dim: Color32,
    pub text_disabled: Color32,
    pub icon: Color32,
    pub hover: Color32,
    pub tool_active: Color32,
    pub accent: Color32,
    pub accent_strong: Color32,
    pub flyout_active: Color32,
    pub row_selected: Color32,
    pub button: Color32,
    pub button_border: Color32,
    pub selection: Color32,
    pub pasteboard: Color32,
    pub radius: u8,
    pub font_size: f32,
}

fn hx(s: u32) -> Color32 {
    Color32::from_rgb((s >> 16) as u8, (s >> 8) as u8, s as u8)
}

impl Tokens {
    /// The Craft look at brightness `b` (Modern scheme).
    pub fn for_brightness(b: Brightness, accent: Accent) -> Self {
        let a = accent.hex();
        let a_strong = accent.strong_hex();
        let a_flyout = accent.flyout_hex();
        let base = Tokens {
            dark: true,
            app_bar: hx(0x0b0d12),
            panel: hx(0x181b22),
            panel_darker: hx(0x13161c),
            tab_strip: hx(0x0f1116),
            input: hx(0x0f1116),
            input_border: hx(0x2c323e),
            border: hx(0x06070a),
            divider: hx(0x232832),
            text: hx(0xdfe4ec),
            text_strong: hx(0xffffff),
            text_dim: hx(0x969faf),
            text_disabled: hx(0x5a6271),
            icon: hx(0xc9d0dc),
            hover: hx(0x262b35),
            tool_active: hx(0x2f3541),
            accent: hx(a),
            accent_strong: hx(a_strong),
            flyout_active: hx(a_flyout),
            row_selected: hx(0x3a2a2c),
            button: hx(0x282d37),
            button_border: hx(0x4a5363),
            // selection follows the accent (was a hardcoded blue 0x4f80ff —
            // the "blue things" on every highlighted row)
            selection: hx(a),
            pasteboard: hx(0x0f1116),
            radius: 4,
            font_size: 13.0,
        };
        match b {
            Brightness::Dark => base,
            // Charcoal, the default.
            Brightness::MediumDark => Tokens {
                app_bar: hx(0x1b1f27),
                panel: hx(0x272c36),
                panel_darker: hx(0x21262f),
                tab_strip: hx(0x1e222a),
                input: hx(0x1a1e26),
                input_border: hx(0x3a414e),
                border: hx(0x11141a),
                divider: hx(0x323946),
                text: hx(0xe4e9f1),
                text_dim: hx(0x99a2b2),
                text_disabled: hx(0x626b7a),
                icon: hx(0xccd3de),
                hover: hx(0x353c49),
                tool_active: hx(0x3d4553),
                row_selected: hx(0x4a3236),
                button: hx(0x39404d),
                button_border: hx(0x5a6476),
                accent: hx(a),
                accent_strong: hx(a_strong),
                flyout_active: hx(a_flyout),
                pasteboard: hx(0x1e222a),
                ..base
            },
            Brightness::MediumLight => Tokens {
                dark: false,
                app_bar: hx(0xc4cad4),
                panel: hx(0xd3d8e0),
                panel_darker: hx(0xc9cfd8),
                tab_strip: hx(0xc4cad4),
                input: hx(0xe9ecf1),
                input_border: hx(0x9aa2b0),
                border: hx(0xa9b0bc),
                divider: hx(0xbcc3ce),
                text: hx(0x1b1f27),
                text_strong: hx(0x000000),
                text_dim: hx(0x4a5160),
                text_disabled: hx(0x7d8594),
                icon: hx(0x272c36),
                hover: hx(0xc5ccd6),
                tool_active: hx(0xb6bec9),
                accent: hx(0xd9472f),
                accent_strong: hx(0xd14a33),
                flyout_active: hx(0xc23e28),
                row_selected: hx(0xe7c4bd),
                button: hx(0xc9d0da),
                button_border: hx(0x8a93a2),
                pasteboard: hx(0xc4cad4),
                ..base
            },
            Brightness::Light => Tokens {
                dark: false,
                app_bar: hx(0xe6e9ee),
                panel: hx(0xf3f5f8),
                panel_darker: hx(0xe9ecf1),
                tab_strip: hx(0xe6e9ee),
                input: hx(0xffffff),
                input_border: hx(0xc3c9d3),
                border: hx(0xd0d5dd),
                divider: hx(0xdde1e8),
                text: hx(0x1b1f27),
                text_strong: hx(0x000000),
                text_dim: hx(0x58606e),
                text_disabled: hx(0x9aa1ad),
                icon: hx(0x2a303b),
                hover: hx(0xe3e7ed),
                tool_active: hx(0xd5dbe3),
                accent: hx(0xd9472f),
                accent_strong: hx(0xd14a33),
                flyout_active: hx(0xc23e28),
                row_selected: hx(0xf8d9d2),
                button: hx(0xe3e7ed),
                button_border: hx(0xa4abb8),
                pasteboard: hx(0xe6e9ee),
                ..base
            },
        }
    }

    /// Mix `self.panel` toward the accent (selected-tab tint).
    pub fn panel_tinted(&self, k: f32) -> Color32 {
        let mix = |p: u8, a: u8| (p as f32 + (a as f32 - p as f32) * k) as u8;
        Color32::from_rgb(mix(self.panel.r(), self.accent.r()), mix(self.panel.g(), self.accent.g()), mix(self.panel.b(), self.accent.b()))
    }
}

impl Default for Tokens {
    fn default() -> Self {
        Self::for_brightness(Brightness::MediumDark, Accent::Coral)
    }
}

pub const CARD_RADIUS: u8 = 8;
pub const FONT_UI: &str = "ui";
pub const FONT_UI_SEMIBOLD: &str = "ui-semibold";
pub const FONT_MONO: &str = "mono";

static THEME: std::sync::RwLock<(Brightness, Accent)> = std::sync::RwLock::new((Brightness::MediumDark, Accent::Coral));

/// Install the theme, at startup and whenever the user picks a look.
pub fn install(ctx: &egui::Context, b: Brightness, accent: Accent) {
    if let Ok(mut g) = THEME.write() {
        *g = (b, accent);
    }
    apply_style(ctx);
}

/// Re-apply the current theme to egui style + fonts (no state change).
pub fn apply_style(ctx: &egui::Context) {
    let (b, accent) = current();
    let t = Tokens::for_brightness(b, accent);
    let mut style = (*ctx.style()).clone();
    let vs = &mut style.visuals;
    *vs = Visuals::dark();
    vs.dark_mode = t.dark;
    vs.override_text_color = Some(t.text);
    vs.window_fill = t.panel;
    vs.panel_fill = t.panel;
    vs.extreme_bg_color = t.input;
    vs.faint_bg_color = t.panel_darker;
    vs.window_stroke = Stroke::new(1.0, t.border);
    vs.widgets.noninteractive.bg_fill = t.panel;
    vs.widgets.noninteractive.fg_stroke = Stroke::new(1.0, t.text);
    vs.widgets.noninteractive.bg_stroke = Stroke::new(1.0, t.divider);
    vs.widgets.inactive.bg_fill = t.button;
    vs.widgets.inactive.fg_stroke = Stroke::new(1.0, t.text_dim);
    vs.widgets.inactive.bg_stroke = Stroke::new(1.0, t.button_border);
    vs.widgets.hovered.bg_fill = t.hover;
    vs.widgets.hovered.fg_stroke = Stroke::new(1.0, t.text);
    vs.widgets.hovered.bg_stroke = Stroke::new(1.0, t.button_border);
    vs.widgets.active.bg_fill = t.tool_active;
    vs.widgets.active.fg_stroke = Stroke::new(1.0, t.text_strong);
    vs.selection.bg_fill = t.accent;
    vs.selection.stroke = Stroke::new(1.0, t.text_strong);
    let r = Rounding::same(t.radius as f32);
    vs.window_rounding = r;
    vs.menu_rounding = r;
    vs.widgets.noninteractive.rounding = r;
    vs.widgets.inactive.rounding = r;
    vs.widgets.hovered.rounding = r;
    vs.widgets.active.rounding = r;
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(8.0, 4.0);
    style.spacing.menu_margin = Margin::same(6.0);
    style.text_styles = [
        (TextStyle::Heading, egui::FontId::proportional(t.font_size + 4.0)),
        (TextStyle::Body, egui::FontId::proportional(t.font_size)),
        (TextStyle::Button, egui::FontId::proportional(t.font_size)),
        (TextStyle::Small, egui::FontId::proportional(t.font_size - 3.0)),
        (TextStyle::Monospace, egui::FontId::monospace(t.font_size - 1.0)),
    ]
    .into();
    ctx.set_style(style);
    fonts(ctx);
}

pub fn current() -> (Brightness, Accent) {
    *THEME.read().unwrap_or_else(|e| e.into_inner())
}

pub fn tokens(ctx: &egui::Context) -> Tokens {
    let (b, a) = current();
    Tokens::for_brightness(b, a)
}

/// VectorCraft-style default fonts (ui / ui-semibold / mono aliases).
fn fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    // Alias families so text style helpers can pick weights cleanly.
    defs.families.entry(FontFamily::Name(FONT_UI.into())).or_default().push("Ubuntu-Light".into());
    defs.families.entry(FontFamily::Name(FONT_MONO.into())).or_default().push("UbuntuMono-Regular".into());
    ctx.set_fonts(defs);
}

/// The card frame panels float in (soft corners, hairline border, drop shadow).
pub fn card_frame(ctx: &egui::Context) -> egui::Frame {
    let t = tokens(ctx);
    egui::Frame::none()
        .fill(t.panel)
        .stroke(Stroke::new(1.0, t.border))
        .rounding(egui::Rounding::same(CARD_RADIUS as f32))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 4.0),
            blur: 16.0,
            spread: 0.0,
            color: Color32::from_black_alpha(80),
        })
        .inner_margin(Margin::same(0.0))
}

/// Paint a card behind `add` (used by floating panels and flyouts).
pub fn card(ui: &mut egui::Ui, margin: Margin, add: impl FnOnce(&mut egui::Ui)) {
    card_frame(ui.ctx()).inner_margin(margin).show(ui, add);
}
