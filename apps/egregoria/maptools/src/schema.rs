//! Compact OSM ways JSON — the exact format used by the egui app
//! (`roadkit::dsm::OsmWay`, files in `traffic-sim/assets/dsm/*.json`).
//!
//! Schema: `{ "ways": [ { "pts": [[lon, lat], ...], "hw": "trunk",
//! "n": "Name", "l": <layer i8>, "b": <bridge u8>, "ln": "2", "ow": "yes" } ] }`

use serde::Deserialize;

#[derive(Deserialize)]
pub struct OsmWays {
    #[serde(default)]
    pub ways: Vec<OsmWay>,
}

#[derive(Deserialize)]
pub struct OsmWay {
    pub pts: Vec<(f64, f64)>,
    pub hw: String,
    #[serde(default)]
    pub n: String,
    #[serde(default)]
    pub l: i8,
    #[serde(default)]
    pub b: u8,
    #[serde(default)]
    pub ln: String,
    #[serde(default, rename = "ow")]
    pub oneway: String,
}

impl OsmWay {
    /// Total lane count from the `ln` tag ("2", "2;1", "1;2;1" -> sum).
    pub fn lanes_total(&self) -> Option<usize> {
        if self.ln.is_empty() {
            return None;
        }
        let sum: usize = self
            .ln
            .split(';')
            .filter_map(|p| p.trim().parse::<usize>().ok())
            .sum();
        if sum == 0 {
            None
        } else {
            Some(sum.min(8))
        }
    }

    pub fn is_oneway(&self) -> bool {
        matches!(self.oneway.as_str(), "yes" | "true" | "1")
    }

    pub fn is_bridge(&self) -> bool {
        self.b > 0 || self.l > 0
    }
}
