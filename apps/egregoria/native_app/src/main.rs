#![allow(clippy::type_complexity)]

#[macro_use]
extern crate common;

extern crate simulation;

#[allow(unused_imports)]
#[macro_use]
extern crate inline_tweak;

#[macro_use]
mod uiworld;

mod audio;
mod debug_gui;
mod game_loop;
mod gui;
mod init;
mod inputmap;
mod network;
mod rendering;

use std::sync::OnceLock;

/// OSM configuration parsed from CLI args.
///
/// Defaults to the embedded Kijazi extracts — the exact same data files the
/// egui app embeds (`traffic-sim/assets/dsm/kijazi.json` + `kijazi.osm`).
#[derive(Clone, Default)]
pub struct OsConfig {
    pub ways_path: Option<String>,
    pub xml_path: Option<String>,
}

pub static OS_CONFIG: OnceLock<OsConfig> = OnceLock::new();

fn parse_args() {
    let mut cfg = OsConfig::default();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--osm" => cfg.ways_path = args.next(),
            "--osm-xml" => cfg.xml_path = args.next(),
            "--help" | "-h" => {
                println!(
                    "egregoria carto — OSM cartography rendered in a 3D environment\n\n\
                     OPTIONS:\n  \
                     --osm <ways.json>       compact OSM ways JSON (default: embedded kijazi.json)\n  \
                     --osm-xml <carto.osm>   Overpass XML: buildings/water/parks (default: embedded kijazi.osm)"
                );
                std::process::exit(0);
            }
            _ => {}
        }
    }
    let _ = OS_CONFIG.set(cfg);
}

fn main() {
    parse_args();

    #[cfg(feature = "profile")]
    profiling::tracy_client::Client::start();
    profiling::register_thread!("Main Thread");

    engine::framework::init();
    init::init();

    engine::framework::start::<game_loop::State>();
}
