//! Theme — thin re-export of the egui-kit design system (the VectorCraft /
//! Illustrator-style look: slate panels, one accent, four brightness levels).

pub use egui_kit::{apply_style, card, current, install, tokens};
pub use egui_kit::{Accent, Brightness, Tokens};

/// Startup install (MediumDark + Coral, the VectorCraft default).
pub fn apply(ctx: &egui::Context) {
    install(ctx, Brightness::MediumDark, Accent::Coral);
}

/// Convenience: the current token set (reads the shared theme state).
pub fn t(ctx: &egui::Context) -> Tokens {
    tokens(ctx)
}
