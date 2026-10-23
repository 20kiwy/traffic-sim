use yakui::widgets::Pad;
use goryak::{mincolumn, on_primary_container, textc, Window};
use maptools::CartoData;
use simulation::Simulation;

use crate::uiworld::UiWorld;

/// "OSM Data" window — shows the road properties imported from the
/// traffic-sim model and the bridge engineering specs, plus carto counts.
pub fn osmdata(uiworld: &UiWorld, _sim: &Simulation, opened: &mut bool) {
    Window {
        title: "OSM Data".into(),
        pad: Pad::all(10.0),
        radius: 10.0,
        opened,
        child_spacing: 0.0,
    }
    .show(|| {
        profiling::scope!("gui::window::osmdata");
        let carto = uiworld.read::<CartoData>();

        mincolumn(0.0, || {
            let (fwd, bwd) = carto
                .props
                .values()
                .fold((0usize, 0usize), |acc, p| (acc.0 + p.lanes_fwd, acc.1 + p.lanes_bwd));

            textc(on_primary_container(), format!("Roads: {}", carto.roads));
            textc(
                on_primary_container(),
                format!("Driving lanes (fwd/bwd): {}/{}", fwd, bwd),
            );
            textc(
                on_primary_container(),
                format!("Bridges: {}", carto.bridges.len()),
            );
            textc(
                on_primary_container(),
                format!("Buildings: {}", carto.buildings),
            );
            textc(
                on_primary_container(),
                format!("Water areas: {}", carto.water.len()),
            );
            textc(
                on_primary_container(),
                format!("Waterways: {}", carto.waterways.len()),
            );
            textc(
                on_primary_container(),
                format!("Landcover areas: {}", carto.areas.len()),
            );

            if !carto.bridges.is_empty() {
                textc(
                    on_primary_container(),
                    "— Bridges (traffic-sim specs) —".to_string(),
                );
                for b in &carto.bridges {
                    let s = &b.spec;
                    let len: f32 = b
                        .pts
                        .windows(2)
                        .map(|w| (w[1].xy() - w[0].xy()).mag2().sqrt())
                        .sum();
                    textc(
                        on_primary_container(),
                        format!(
                            "{} · {:?} · deck {:.1} m · clearance {:.1} m · rails {:.1} m · {:.0} m · layer {}",
                            if b.name.is_empty() {
                                "Bridge".to_string()
                            } else {
                                b.name.clone()
                            },
                            s.kind,
                            s.deck_width_m,
                            s.clearance_m,
                            s.rail_h_m,
                            len,
                            s.layer,
                        ),
                    );
                }
            }
        });
    });
}
