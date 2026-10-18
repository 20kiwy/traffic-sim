//! egui-kit — the RoadKit design system.
//!
//! A faithful port of VectorCraft's Illustrator-style chrome (theme tokens, icon
//! painting, panel widgets), adapted to egui 0.28. Everything an app needs to look
//! like a professional creative tool: four brightness levels, one accent colour,
//! flat slate panels with soft corners.

mod icons;
mod theme;
mod widgets;

pub use icons::{icon, paint, IconName};
pub use theme::{apply_style, card, current, install, tokens, Accent, Brightness, Tokens, CARD_RADIUS, FONT_MONO, FONT_UI, FONT_UI_SEMIBOLD};
pub use widgets::{drag_value_labeled_f64,
    btn, checkbox_row, color_swatch, combo, collapse_header, dim_text, drag_value_labeled, eye_lock_columns,
    floating_window, icon_button, icon_button_caption, icon_button_flat, row_selected_bg, search_field, section, tab_bar, tree_row,
    TreeRowStyle,
};
