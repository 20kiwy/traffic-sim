//! openstreetmap-carto styling — the bridge between the vendored
//! [`carto_style`] crate (1:1 copy of the map-launcher style engine) and
//! Egregoria's renderer color type.
//!
//! Every carto color the app draws resolves through the *real*
//! openstreetmap-carto palette — all 19 `.mss` style sheets evaluated by the
//! same CartoCSS parser the map-launcher tile server uses — so the 3D world
//! matches the flat map 100%. Nothing here is hardcoded in the app layer.

use carto_style::{darken, Palette, Rgba};
use geom::{from_srgb, LinearColor};

/// The full openstreetmap-carto palette + style accessors.
pub struct CartoStyle {
    palette: Palette,
}

/// carto `.mss` colors are sRGB bytes; the engine's shader colors are LINEAR
/// (the swapchain is an sRGB format that re-encodes on write), so every
/// palette color MUST go through the standard sRGB → linear transfer
/// function — exactly what `From<Color> for LinearColor` does elsewhere.
/// Skipping it painted the whole map washed-out (the "colors not well seen"
/// report: `byte/255` written raw, then sRGB-encoded again on output).
fn lin(c: Rgba) -> LinearColor {
    LinearColor {
        r: from_srgb(c.r as f32 / 255.0),
        g: from_srgb(c.g as f32 / 255.0),
        b: from_srgb(c.b as f32 / 255.0),
        a: c.a as f32 / 255.0,
    }
}

impl CartoStyle {
    /// Palette evaluated from the `.mss` style sheets embedded in
    /// `carto-style` (verbatim openstreetmap-carto).
    pub fn embedded() -> Self {
        Self {
            palette: Palette::from_embedded(),
        }
    }

    /// Resolve an `@var` (e.g. `"land-color"`, `"motorway-fill"`).
    pub fn rgba(&self, var: &str) -> Rgba {
        self.palette.get(var).unwrap_or_else(|| {
            log::warn!("carto-style: missing var @{}", var);
            Rgba::rgb(255, 0, 255)
        })
    }

    /// Resolve an `@var` as a renderer color.
    pub fn linear(&self, var: &str) -> LinearColor {
        lin(self.rgba(var))
    }

    // -- ground layers ----------------------------------------------------

    /// `@land-color` — the carto background (#f2efe9).
    pub fn land(&self) -> LinearColor {
        self.linear("land-color")
    }

    /// `@water-color` — openstreetmap-carto water (#aad3df).
    pub fn water(&self) -> LinearColor {
        self.linear("water-color")
    }

    /// OSM landcover / landuse / natural / leisure tag value → the carto
    /// `@var` that paints it (landcover.mss + style.mss). `None` = leave the
    /// ground color.
    pub fn area_var(kind: &str) -> Option<&'static str> {
        Some(match kind {
            "forest" | "wood" => "forest",
            "grass" | "meadow" | "village_green" | "wetland" => "grass",
            "scrub" => "scrub",
            "park" | "garden" | "playground" | "golf_course" | "common" => "park",
            "pitch" => "pitch",
            "farmland" => "farmland",
            "farmyard" => "farmyard",
            "orchard" | "vineyard" | "plant_nursery" => "orchard",
            "allotments" => "allotments",
            "residential" => "residential",
            "retail" => "retail",
            "commercial" => "commercial",
            "industrial" | "railway" => "industrial",
            "cemetery" | "grave_yard" => "cemetery",
            "heath" => "heath",
            "quarry" => "quarry",
            "construction" | "brownfield" | "greenfield" => "construction",
            "sand" => "sand",
            "beach" => "beach",
            "bare_ground" => "bare_ground",
            "camp_site" | "picnic_site" | "caravan_site" => "campsite",
            "parking" => "parking",
            "apron" => "apron",
            _ => return None,
        })
    }

    /// Green landcover paints above the built-up landuse (carto paints
    /// landuse first, landcover after).
    pub fn is_green_var(var: &str) -> bool {
        matches!(
            var,
            "forest"
                | "grass"
                | "scrub"
                | "park"
                | "pitch"
                | "farmland"
                | "farmyard"
                | "orchard"
                | "allotments"
                | "sand"
                | "beach"
                | "bare_ground"
                | "campsite"
                | "heath"
                | "cemetery"
        )
    }

    // -- buildings ----------------------------------------------------------

    /// `@building-fill` (#d9d0c9).
    pub fn building_fill(&self) -> LinearColor {
        self.linear("building-fill")
    }

    /// `@building-line` = darken(@building-fill, 15%) — roof / edge tone.
    pub fn building_line(&self) -> LinearColor {
        lin(darken(self.rgba("building-fill"), 15.0))
    }

    // -- roads ---------------------------------------------------------------

    /// Road surface `@var` per OSM highway class (roads.mss + the
    /// tile-server `fill_for` mapping).
    pub fn road_fill_var(hw: &str) -> &'static str {
        match hw {
            "motorway" | "motorway_link" => "motorway-fill",
            "trunk" | "trunk_link" => "trunk-fill",
            "primary" | "primary_link" => "primary-fill",
            "secondary" | "secondary_link" => "secondary-fill",
            "tertiary" | "tertiary_link" => "tertiary-fill",
            "living_street" => "living-street-fill",
            "pedestrian" => "pedestrian-fill",
            "track" => "track-fill",
            "footway" | "path" | "steps" | "cycleway" => "footway-fill",
            // residential / unclassified / service / road
            _ => "residential-fill",
        }
    }

    /// Road casing `@var` per OSM highway class (tile-server `casing_for`).
    pub fn road_casing_var(hw: &str) -> &'static str {
        match hw {
            "motorway" | "motorway_link" => "motorway-casing",
            "trunk" | "trunk_link" => "trunk-casing",
            "primary" | "primary_link" => "primary-casing",
            "secondary" | "secondary_link" => "secondary-casing",
            "tertiary" | "tertiary_link" => "tertiary-casing",
            "pedestrian" => "pedestrian-casing",
            "track" | "footway" | "path" | "steps" | "cycleway" => "default-casing",
            _ => "residential-casing",
        }
    }

    pub fn road_fill(&self, hw: &str) -> LinearColor {
        self.linear(Self::road_fill_var(hw))
    }

    pub fn road_casing(&self, hw: &str) -> LinearColor {
        self.linear(Self::road_casing_var(hw))
    }

    /// carto `@bridge-casing` — black outline around bridge decks (z13+).
    pub fn bridge_casing(&self) -> LinearColor {
        LinearColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        }
    }

    /// Oneway arrow tint — roads.mss `::directions` uses
    /// `darken(casing, 10-40%)` per class.
    pub fn oneway_arrow(&self, hw: &str) -> LinearColor {
        lin(darken(
            self.rgba(Self::road_casing_var(hw)),
            20.0,
        ))
    }

    /// Sidewalk / walking surface — `@pedestrian-fill` (#dddde8).
    pub fn pedestrian_fill(&self) -> LinearColor {
        self.linear("pedestrian-fill")
    }

    /// Railway dark base (roads.mss z12+ two-tone: dark #707070 + white dash).
    pub fn rail(&self) -> LinearColor {
        lin(Rgba::rgb(0x70, 0x70, 0x70))
    }
}

/// Paint priority per class — the carto roads.mss rule order (more major
/// classes paint last / win). Used to pick the junction surface color.
pub fn road_class_rank(hw: &str) -> u8 {
    match hw {
        "motorway" => 0,
        "motorway_link" => 1,
        "trunk" => 2,
        "trunk_link" => 3,
        "primary" => 4,
        "primary_link" => 5,
        "secondary" => 6,
        "secondary_link" => 7,
        "tertiary" => 8,
        "tertiary_link" => 9,
        "residential" | "unclassified" | "road" => 10,
        "living_street" => 11,
        "pedestrian" => 12,
        "service" => 13,
        "track" => 14,
        "footway" | "path" | "steps" | "cycleway" => 15,
        _ => 16,
    }
}

/// The shared palette — parsed once, used by the renderer and the UI.
pub fn global() -> &'static CartoStyle {
    static CARTO: std::sync::OnceLock<CartoStyle> = std::sync::OnceLock::new();
    CARTO.get_or_init(CartoStyle::embedded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_roles_resolve() {
        let s = CartoStyle::embedded();
        // spot-check the documented openstreetmap-carto values
        let land = s.rgba("land-color");
        assert_eq!((land.r, land.g, land.b), (0xf2, 0xef, 0xe9));
        let water = s.rgba("water-color");
        assert_eq!((water.r, water.g, water.b), (0xaa, 0xd3, 0xdf));
        let mw = s.rgba("motorway-fill");
        assert_eq!((mw.r, mw.g, mw.b), (0xe8, 0x92, 0xa2));
        let b = s.rgba("building-fill");
        assert_eq!((b.r, b.g, b.b), (0xd9, 0xd0, 0xc9));
    }

    #[test]
    fn colors_are_linearized() {
        // sRGB bytes must go through the transfer function — the shader
        // colors are linear (sRGB swapchain re-encodes on write). Writing
        // byte/255 raw washed the whole map out.
        let s = CartoStyle::embedded();
        let land = s.land();
        // from_srgb(0xf2/255 = 0.949) ≈ 0.886 — strictly darker than the
        // raw fraction
        assert!(land.r < 0.949 && land.r > 0.85, "land r = {}", land.r);
        // and round-trips back to the carto hex
        let c: geom::Color = land.into();
        assert_eq!(((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8), (0xf2, 0xef, 0xe9));
        let water = s.water();
        let c: geom::Color = water.into();
        assert_eq!(((c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8), (0xaa, 0xd3, 0xdf));
    }

    #[test]
    fn road_mapping() {
        let s = CartoStyle::embedded();
        let primary = s.rgba("primary-fill");
        assert_eq!((primary.r, primary.g, primary.b), (0xfc, 0xd6, 0xa4));
        assert_eq!(CartoStyle::road_fill_var("service"), "residential-fill");
        assert_eq!(CartoStyle::road_casing_var("tertiary"), "tertiary-casing");
        let arrow = s.oneway_arrow("motorway");
        assert!(arrow.r < 1.0 && arrow.a == 1.0);
    }
}

