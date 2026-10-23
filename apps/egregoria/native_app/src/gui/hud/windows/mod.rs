pub mod layers;
pub mod load;
pub mod osmdata;
pub mod settings;

use crate::uiworld::UiWorld;
use goryak::button_primary;
use simulation::Simulation;

#[cfg(feature = "multiplayer")]
pub mod network;

#[derive(Default)]
pub struct GUIWindows {
    osmdata_open: bool,
    layers_open: bool,
    settings_open: bool,
    load_open: bool,
    #[cfg(feature = "multiplayer")]
    network_open: bool,
}

impl GUIWindows {
    pub fn new() -> Self {
        Self {
            // EGRE_LAYERS=1 opens the carto layer-engine panel on start
            // (headless verification / first-run discoverability).
            layers_open: std::env::var("EGRE_LAYERS").is_ok(),
            ..Default::default()
        }
    }
}

impl GUIWindows {
    pub fn menu(&mut self) {
        // The Economy window was removed together with the economy systems.

        if button_primary("Layers").show().clicked {
            self.layers_open ^= true;
        }

        if button_primary("OSM Data").show().clicked {
            self.osmdata_open ^= true;
        }

        if button_primary("Settings").show().clicked {
            self.settings_open ^= true;
        }

        if button_primary("Load").show().clicked {
            self.load_open ^= true;
        }

        #[cfg(feature = "multiplayer")]
        if button_primary("Network").show().clicked {
            self.network_open ^= true;
        }
    }

    pub fn render(&mut self, uiworld: &UiWorld, sim: &Simulation) {
        profiling::scope!("windows::render");

        layers::layers(uiworld, sim, &mut self.layers_open);
        osmdata::osmdata(uiworld, sim, &mut self.osmdata_open);
        settings::settings(uiworld, sim, &mut self.settings_open);
        load::load(uiworld, sim, &mut self.load_open);

        #[cfg(feature = "multiplayer")]
        network::network(uiworld, sim, &mut self.network_open);
    }
}
