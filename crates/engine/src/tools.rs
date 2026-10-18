//! The land toolbars — hardcoded as a lib from egregoria
//! (`native_app/src/gui/hud/toolbox/mod.rs` + `gui/tools/*.rs`, MIT).
//!
//! The layout is copied verbatim in spirit: a TOP-LEFT pinned column with a
//! per-tool properties strip (blur panel) above a row of 64×64 icon buttons
//! (blur panel), selection marked with a small triangle underline. Tools are
//! plain functions on the world — there is no Tool trait, matching upstream.

use crate::pem::TerraformKind;

/// The Tool enum copied from egregoria `native_app/src/gui/mod.rs`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Hand,
    RoadbuildStraight,
    RoadbuildCurved,
    RoadEditor,
    Bulldozer,
    LotBrush,
    SpecialBuilding,
    Train,
    Terraforming,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Hand => "Hand",
            Tool::RoadbuildStraight => "Straight road",
            Tool::RoadbuildCurved => "Curved road",
            Tool::RoadEditor => "Road editor",
            Tool::Bulldozer => "Bulldozer",
            Tool::LotBrush => "Zone brush",
            Tool::SpecialBuilding => "Special building",
            Tool::Train => "Trains",
            Tool::Terraforming => "Terraforming",
        }
    }
    pub fn is_roadbuild(self) -> bool {
        matches!(self, Tool::RoadbuildStraight | Tool::RoadbuildCurved)
    }
}

/// The toolbox table — exactly the icon -> tool mapping of egregoria's
/// `new_toolbox` (icons resolve against `assets/ui/icons/<name>.png`).
pub const TOOLS: [(&str, Tool); 8] = [
    ("toolbar_straight_road", Tool::RoadbuildStraight),
    ("toolbar_curved_road", Tool::RoadbuildCurved),
    ("toolbar_road_edit", Tool::RoadEditor),
    ("toolbar_housetool", Tool::LotBrush),
    ("toolbar_companies", Tool::SpecialBuilding),
    ("toolbar_bulldozer", Tool::Bulldozer),
    ("toolbar_train", Tool::Train),
    ("toolbar_terraform", Tool::Terraforming),
];

/// Overlay draw orders, ported from egregoria's `ImmediateDraw`
/// (`OrderKind::{Circle, StrokeCircle, Line, PolyLine, Polygon, OBB,
/// TexturedOBB, Mesh}`) with builder-chaining colors.
#[derive(Clone, Debug)]
pub enum Order {
    Circle {
        center: [f64; 2],
        radius: f64,
        color: [f32; 4],
    },
    StrokeCircle {
        center: [f64; 2],
        radius: f64,
        width: f64,
        color: [f32; 4],
    },
    Line {
        a: [f64; 3],
        b: [f64; 3],
        color: [f32; 4],
    },
    PolyLine {
        pts: Vec<[f64; 3]>,
        color: [f32; 4],
    },
    Polygon {
        pts: Vec<[f64; 3]>,
        color: [f32; 4],
    },
    OBB {
        pos: [f64; 2],
        yaw: f64,
        half: [f64; 2],
        color: [f32; 4],
    },
}

/// Overlay buffer a tool fills every frame (egregoria `ImmediateDraw`).
#[derive(Default)]
pub struct ImmediateDraw {
    pub orders: Vec<Order>,
}

impl ImmediateDraw {
    pub fn circle(&mut self, center: [f64; 2], radius: f64, color: [f32; 4]) {
        self.orders.push(Order::Circle { center, radius, color });
    }
    pub fn stroke_circle(&mut self, center: [f64; 2], radius: f64, width: f64, color: [f32; 4]) {
        self.orders.push(Order::StrokeCircle { center, radius, width, color });
    }
    pub fn poly_line(&mut self, pts: Vec<[f64; 3]>, color: [f32; 4]) {
        self.orders.push(Order::PolyLine { pts, color });
    }
    pub fn obb(&mut self, pos: [f64; 2], yaw: f64, half: [f64; 2], color: [f32; 4]) {
        self.orders.push(Order::OBB { pos, yaw, half, color });
    }
}

/// gui color tokens (egregoria gui_primary/gui_secondary on blur panels).
pub const GUI_PRIMARY: [f32; 4] = [0.20, 0.79, 0.57, 0.85];
pub const GUI_SECONDARY: [f32; 4] = [0.16, 0.17, 0.19, 0.72];
pub const GUI_DANGER: [f32; 4] = [0.86, 0.26, 0.22, 0.85];
pub const GUI_TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];

/// Per-tool properties values, the state the properties strip edits
/// (egregoria: TerraformingResource / RoadBuildResource equivalents).
#[derive(Copy, Clone, Debug)]
pub struct ToolOptions {
    /// terraform: selected kind index into TerraformKind::ALL
    pub terraform_kind: usize,
    /// terraform radius step (3 sizes like the upstream panel)
    pub terraform_size: f64,
    /// terraform amount step
    pub terraform_amount: f32,
    /// road build: road type index into the toolbox road-type row
    pub road_type: usize,
    /// height reference mode 0=ground 1=start 2=max incline 3=max decline
    pub height_ref: usize,
    /// snap mode 0=none 1=grid 2=angle
    pub snap: usize,
    /// roundabout radius (roadedit)
    pub roundabout_radius: f64,
}

impl Default for ToolOptions {
    fn default() -> Self {
        ToolOptions {
            terraform_kind: 0,
            terraform_size: 50.0,
            terraform_amount: 5.0,
            road_type: 0,
            height_ref: 0,
            snap: 0,
            roundabout_radius: 8.0,
        }
    }
}

/// The world commands tools emit (egregoria `WorldCommand` subset that
/// matters for land editing in traffic-sim).
#[derive(Clone, Debug)]
pub enum WorldCommand {
    Terraform {
        kind: TerraformKind,
        center: (f64, f64),
        radius: f64,
        amount: f32,
    },
    StampRoads {
        /// polyline (x, z, elev)
        pts: Vec<(f64, f64, f64)>,
        width: f64,
    },
}

/// the toolbox layout constants (px) copied from the upstream HUD
pub const TOOLBOX_ICON: f32 = 64.0;
pub const TOOLBOX_PAD: f32 = 6.0;
pub const TOOLBOX_PANEL_ALPHA: f32 = 0.72;
