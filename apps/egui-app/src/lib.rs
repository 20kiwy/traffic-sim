//! egui-app — the editor application crate.
//!
//! Exposes the app modules as a library so `roadkit-cli` (headless screenshots,
//! `cargo xtask dist`) reuses the exact same script bridge and scene pipeline.
//! The binary is the eframe GUI.

pub mod actions;
pub mod bridge;
pub mod chrome;
pub mod history;
pub mod nodes_schema;
pub mod panels;
pub mod planview;
pub mod scripts;
pub mod selftest;
pub mod state;
pub mod statusbar;
pub mod theme;
pub mod viewport3d;
