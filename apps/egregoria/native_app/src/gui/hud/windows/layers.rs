use yakui::widgets::{List, Pad};
use yakui::{divider, MainAxisSize, Vec2};

use goryak::{
    button_primary, checkbox_value, minrow, on_primary_container, on_secondary_container, outline,
    padx, textc, VertScrollSize, Window,
};
use maptools::layers::{layer_catalog, LayerVis};
use simulation::Simulation;

use crate::uiworld::UiWorld;

/// Catalog ids the 3D renderer actually paints. Every other row still shows
/// (100% parity with the carto preview tile-server's layer tree) but is
/// marked "no 3D data" — the same as an empty layer at the current zoom on
/// the flat map.
const DATA_BOUND: &[&str] = &[
    "background",
    "landuse",
    "water-areas",
    "waterways",
    "ocean",
    "road-casing",
    "road-fill",
    "road-oneway",
    "bridges",
    "rail",
    "buildings",
    "trees",
];

fn depth_of(cat: &[maptools::layers::LayerDef], d: &maptools::layers::LayerDef) -> usize {
    let mut cur = d.parent;
    let mut depth = 0;
    while !cur.is_empty() {
        cur = match cat.iter().find(|x| x.id == cur) {
            Some(p) => p.parent,
            None => break,
        };
        depth += 1;
    }
    depth
}

/// "Layers" window — the map-launcher tile-server's layer-engine panel,
/// copied 1:1 (catalog, tree structure, eye toggles, swatches) and wired to
/// the 3D renderer through `LayerVis`.
pub fn layers(uiworld: &UiWorld, _sim: &Simulation, opened: &mut bool) {
    Window {
        title: "Layers".into(),
        pad: Pad::all(10.0),
        radius: 10.0,
        opened,
        child_spacing: 0.0,
    }
    .show(|| {
        profiling::scope!("gui::window::layers");
        let mut vis = uiworld.write::<LayerVis>();

        minrow(8.0, || {
            if button_primary("Show all").show().clicked {
                vis.show_all();
            }
            if button_primary("Hide all").show().clicked {
                vis.hide_all();
            }
        });

        divider(outline(), 10.0, 1.0);

        VertScrollSize::Percent(0.7).show(|| {
            let mut l = List::column();
            l.item_spacing = 2.0;
            l.main_axis_size = MainAxisSize::Min;
            l.show(|| {
                let cat = layer_catalog();
                for d in &cat {
                    let depth = depth_of(&cat, d);
                    // a row paints in 3D when it is itself data-bound or is
                    // a group holding data-bound descendants (the group eye
                    // toggles them all through LayerVis::on)
                    let has_data = DATA_BOUND.contains(&d.id)
                        || maptools::layers::descendants_of(&cat, d.id)
                            .iter()
                            .any(|id| DATA_BOUND.contains(id));
                    let is_group = cat.iter().any(|x| x.parent == d.id);

                    padx(depth as f32 * 14.0, || {
                        minrow(4.0, || {
                            // colour swatch — the palette var the carto
                            // style sheet paints this layer with (raw sRGB
                            // bytes — UI swatches are display-space)
                            let swatch = match d.color_var {
                                Some(var) => {
                                    let r = maptools::carto::global().rgba(var);
                                    yakui::Color::rgba(r.r, r.g, r.b, r.a)
                                }
                                None => yakui::Color::rgb(
                                    d.fallback[0],
                                    d.fallback[1],
                                    d.fallback[2],
                                ),
                            };
                            yakui::colored_box(swatch, Vec2::splat(12.0));

                            let mut on = vis.on(d.id);
                            checkbox_value(
                                &mut on,
                                if is_group {
                                    on_primary_container()
                                } else {
                                    on_secondary_container()
                                },
                                d.name,
                            );
                            if on != vis.on(d.id) {
                                vis.set_visible(d.id, on);
                            }

                            if !has_data {
                                textc(on_secondary_container().with_alpha(0.55), "no 3D data");
                            }
                        });
                    });
                }
            });
        });

        divider(outline(), 10.0, 1.0);
        textc(
            on_secondary_container(),
            format!(
                "{} layers · openstreetmap-carto (carto preview tile-server catalog)",
                layer_catalog().len(),
            ),
        );
    });
}
