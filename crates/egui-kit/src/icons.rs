//! Icon lookup and drawing — painted directly with egui primitives (VectorCraft
//! rasterizes SVGs; here the same lucide-style 24×24 grid is drawn with strokes so
//! the app stays dependency-free and resolution independent).

use egui::{pos2, vec2, Color32, Pos2, Rect, Stroke, Ui, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconName {
    Eye,
    EyeOff,
    Lock,
    Unlock,
    ChevronRight,
    ChevronDown,
    ChevronsRight,
    ChevronsLeft,
    Search,
    Close,
    Plus,
    Minus,
    Trash,
    Layers,
    Sliders,
    Box,
    Script,
    Console,
    Camera,
    Play,
    Cursor,
    Pen,
    Stamp,
    Hand,
    Road,
    Point,
    Junction,
    TrafficLight,
    Sign,
    Bus,
    Grid,
    Circle,
    Save,
    Download,
}

impl IconName {
    pub fn label(self) -> &'static str {
        match self {
            IconName::Eye => "eye",
            IconName::EyeOff => "eye-off",
            IconName::Lock => "lock",
            IconName::Unlock => "unlock",
            IconName::ChevronRight => "chevron-right",
            IconName::ChevronDown => "chevron-down",
            IconName::ChevronsRight => "chevrons-right",
            IconName::ChevronsLeft => "chevrons-left",
            IconName::Search => "search",
            IconName::Close => "x",
            IconName::Plus => "plus",
            IconName::Minus => "minus",
            IconName::Trash => "trash",
            IconName::Layers => "layers",
            IconName::Sliders => "sliders",
            IconName::Box => "box",
            IconName::Script => "script",
            IconName::Console => "console",
            IconName::Camera => "camera",
            IconName::Play => "play",
            IconName::Cursor => "cursor",
            IconName::Pen => "pen-tool",
            IconName::Stamp => "stamp",
            IconName::Hand => "hand",
            IconName::Road => "road",
            IconName::Point => "map-pin",
            IconName::Junction => "git-merge",
            IconName::TrafficLight => "traffic-cone",
            IconName::Sign => "sign",
            IconName::Bus => "bus",
            IconName::Grid => "grid",
            IconName::Circle => "circle",
            IconName::Save => "save",
            IconName::Download => "download",
        }
    }
}

/// Paint icon `name` into `rect`, tinted with `tint`. Drawn on a 24×24 design grid
/// scaled to the rect, stroke width 1.6 (VectorCraft's SvgStyle).
pub fn paint(ui: &Ui, name: IconName, rect: Rect, tint: Color32) {
    let p = ui.painter();
    let s = rect.width().min(rect.height());
    let off = rect.center() - Vec2::splat(s / 2.0);
    let sc = s / 24.0;
    let w = 1.6 * sc.max(0.6);
    let st = Stroke::new(w, tint);
    let mut pts = |arr: &[(f64, f64)]| -> Vec<Pos2> { arr.iter().map(|(x, y)| off + vec2(*x as f32 * sc, *y as f32 * sc)).collect() };
    let l = |a: (f64, f64), b: (f64, f64)| {
        p.line_segment([off + vec2(a.0 as f32 * sc, a.1 as f32 * sc), off + vec2(b.0 as f32 * sc, b.1 as f32 * sc)], st)
    };
    let c = |center: (f64, f64), r: f64, fill: bool| {
        let ctr = off + vec2(center.0 as f32 * sc, center.1 as f32 * sc);
        if fill {
            p.circle_filled(ctr, (r * sc as f64) as f32, tint);
        } else {
            p.circle_stroke(ctr, (r * sc as f64) as f32, st);
        }
    };
    match name {
        IconName::Eye => {
            pts(&[]);
            c((12.0, 12.0), 3.0, false);
            p.add(egui::Shape::line(pts(&[(2.0, 12.0), (7.5, 12.0)]).into(), st));
            l((16.5, 12.0), (22.0, 12.0));
            // upper + lower lids
            p.add(egui::Shape::line(pts(&[(4.0, 8.0), (7.0, 5.5), (12.0, 5.0), (17.0, 5.5), (20.0, 8.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(4.0, 16.0), (7.0, 18.5), (12.0, 19.0), (17.0, 18.5), (20.0, 16.0)]).into(), st));
        }
        IconName::EyeOff => {
            l((4.0, 4.0), (20.0, 20.0));
            c((12.0, 12.0), 3.0, false);
            p.add(egui::Shape::line(pts(&[(6.0, 7.0), (4.5, 9.0), (4.0, 12.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(20.0, 12.0), (19.0, 9.0), (17.0, 6.5)]).into(), st));
        }
        IconName::Lock => {
            p.rect_filled(Rect::from_min_size(off + vec2(6.0 * sc, 11.0 * sc), vec2(12.0 * sc, 8.0 * sc)), 1.5 * sc, tint);
            p.add(egui::Shape::line(pts(&[(8.0, 11.0), (8.0, 8.0), (10.0, 5.5), (14.0, 5.5), (16.0, 8.0), (16.0, 11.0)]).into(), st));
        }
        IconName::Unlock => {
            p.rect_filled(Rect::from_min_size(off + vec2(6.0 * sc, 11.0 * sc), vec2(12.0 * sc, 8.0 * sc)), 1.5 * sc, tint);
            p.add(egui::Shape::line(pts(&[(8.0, 11.0), (8.0, 8.0), (10.0, 5.5), (14.0, 5.5), (16.0, 7.5)]).into(), st));
        }
        IconName::ChevronRight => {
            p.add(egui::Shape::line(pts(&[(9.0, 5.0), (16.0, 12.0), (9.0, 19.0)]).into(), st));
        }
        IconName::ChevronDown => {
            p.add(egui::Shape::line(pts(&[(5.0, 9.0), (12.0, 16.0), (19.0, 9.0)]).into(), st));
        }
        IconName::ChevronsRight => {
            p.add(egui::Shape::line(pts(&[(6.0, 6.0), (12.0, 12.0), (6.0, 18.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(13.0, 6.0), (19.0, 12.0), (13.0, 18.0)]).into(), st));
        }
        IconName::ChevronsLeft => {
            p.add(egui::Shape::line(pts(&[(18.0, 6.0), (12.0, 12.0), (18.0, 18.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(11.0, 6.0), (5.0, 12.0), (11.0, 18.0)]).into(), st));
        }
        IconName::Search => {
            c((10.5, 10.5), 5.5, false);
            l((14.8, 14.8), (20.0, 20.0));
        }
        IconName::Close => {
            l((6.0, 6.0), (18.0, 18.0));
            l((18.0, 6.0), (6.0, 18.0));
        }
        IconName::Plus => {
            l((12.0, 5.0), (12.0, 19.0));
            l((5.0, 12.0), (19.0, 12.0));
        }
        IconName::Minus => {
            l((5.0, 12.0), (19.0, 12.0));
        }
        IconName::Trash => {
            l((4.0, 7.0), (20.0, 7.0));
            l((9.0, 7.0), (9.0, 4.5));
            l((15.0, 7.0), (15.0, 4.5));
            p.add(egui::Shape::line(pts(&[(6.0, 7.0), (7.5, 20.0), (16.5, 20.0), (18.0, 7.0)]).into(), st));
            l((10.0, 11.0), (10.0, 16.0));
            l((14.0, 11.0), (14.0, 16.0));
        }
        IconName::Layers => {
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(12.0 * sc, 4.0 * sc), off + vec2(21.0 * sc, 9.0 * sc), off + vec2(12.0 * sc, 14.0 * sc), off + vec2(3.0 * sc, 9.0 * sc)],
                tint.gamma_multiply(0.35),
                st,
            ));
            p.add(egui::Shape::line(pts(&[(3.0, 13.0), (12.0, 18.0), (21.0, 13.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(3.0, 17.0), (12.0, 22.0), (21.0, 17.0)]).into(), st));
        }
        IconName::Sliders => {
            l((5.0, 5.0), (5.0, 9.0));
            l((5.0, 14.0), (5.0, 19.0));
            c((5.0, 11.5), 2.2, false);
            l((12.0, 5.0), (12.0, 7.0));
            l((12.0, 12.0), (12.0, 19.0));
            c((12.0, 9.5), 2.2, false);
            l((19.0, 5.0), (19.0, 13.0));
            l((19.0, 18.0), (19.0, 19.0));
            c((19.0, 15.5), 2.2, false);
        }
        IconName::Box => {
            p.add(egui::Shape::line(pts(&[(3.0, 8.0), (12.0, 3.0), (21.0, 8.0), (21.0, 17.0), (12.0, 21.5), (3.0, 17.0), (3.0, 8.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(3.0, 8.0), (12.0, 12.5), (21.0, 8.0)]).into(), st));
            l((12.0, 12.5), (12.0, 21.5));
        }
        IconName::Script => {
            p.add(egui::Shape::line(pts(&[(7.0, 3.5), (17.0, 3.5), (19.5, 6.0), (19.5, 20.5), (7.0, 20.5), (4.5, 18.0), (4.5, 6.0), (7.0, 3.5)]).into(), st));
            l((9.0, 9.0), (15.0, 9.0));
            l((9.0, 12.5), (15.0, 12.5));
            l((9.0, 16.0), (13.0, 16.0));
        }
        IconName::Console => {
            p.add(egui::Shape::rect_filled(Rect::from_min_size(off + vec2(3.0 * sc, 5.0 * sc), vec2(18.0 * sc, 14.0 * sc)), 1.5 * sc, tint.gamma_multiply(0.25)));
            p.rect_stroke(Rect::from_min_size(off + vec2(3.0 * sc, 5.0 * sc), vec2(18.0 * sc, 14.0 * sc)), 1.5 * sc, st);
            p.add(egui::Shape::line(pts(&[(7.0, 9.5), (10.0, 12.0), (7.0, 14.5)]).into(), st));
            l((12.0, 15.0), (17.0, 15.0));
        }
        IconName::Camera => {
            p.rect_stroke(Rect::from_min_size(off + vec2(3.0 * sc, 7.0 * sc), vec2(18.0 * sc, 12.0 * sc)), 2.0 * sc, st);
            c((12.0, 13.0), 3.5, false);
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(8.0 * sc, 7.0 * sc), off + vec2(9.5 * sc, 4.5 * sc), off + vec2(14.5 * sc, 4.5 * sc), off + vec2(16.0 * sc, 7.0 * sc)],
                tint,
                Stroke::NONE,
            ));
        }
        IconName::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(8.0 * sc, 5.0 * sc), off + vec2(19.0 * sc, 12.0 * sc), off + vec2(8.0 * sc, 19.0 * sc)],
                tint,
                Stroke::NONE,
            ));
        }
        IconName::Cursor => {
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(6.0 * sc, 3.0 * sc), off + vec2(6.0 * sc, 19.0 * sc), off + vec2(10.5 * sc, 14.5 * sc), off + vec2(13.5 * sc, 21.0 * sc), off + vec2(15.8 * sc, 19.8 * sc), off + vec2(13.0 * sc, 13.5 * sc), off + vec2(19.0 * sc, 13.0 * sc)],
                tint,
                Stroke::new(w * 0.8, Color32::BLACK),
            ));
        }
        IconName::Pen => {
            p.add(egui::Shape::line(pts(&[(12.0, 5.0), (19.0, 12.0), (12.0, 19.0), (5.0, 12.0), (12.0, 5.0)]).into(), st));
            c((12.0, 12.0), 2.0, false);
            l((12.0, 3.5), (12.0, 5.0));
            l((20.5, 12.0), (19.0, 12.0));
            l((12.0, 20.5), (12.0, 19.0));
            l((3.5, 12.0), (5.0, 12.0));
        }
        IconName::Stamp => {
            p.add(egui::Shape::rect_filled(Rect::from_min_size(off + vec2(5.0 * sc, 16.0 * sc), vec2(14.0 * sc, 4.0 * sc)), 1.0 * sc, tint));
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(9.0 * sc, 15.0 * sc), off + vec2(15.0 * sc, 15.0 * sc), off + vec2(14.0 * sc, 9.0 * sc), off + vec2(15.5 * sc, 6.5 * sc), off + vec2(12.0 * sc, 4.0 * sc), off + vec2(8.5 * sc, 6.5 * sc), off + vec2(10.0 * sc, 9.0 * sc)],
                tint,
                Stroke::NONE,
            ));
        }
        IconName::Hand => {
            p.add(egui::Shape::line(pts(&[(8.0, 12.0), (8.0, 5.5), (9.6, 4.4), (11.0, 5.5), (11.0, 11.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(11.0, 11.0), (11.0, 4.5), (12.6, 3.6), (14.0, 4.8), (14.0, 11.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(14.0, 11.0), (14.0, 6.0), (15.7, 5.4), (17.0, 6.5), (17.0, 12.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(8.0, 12.0), (7.0, 10.5), (5.6, 11.0), (5.4, 13.0), (7.0, 17.5), (9.5, 20.0), (14.5, 20.0), (17.0, 17.0), (17.0, 12.0)]).into(), st));
        }
        IconName::Road => {
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(7.0 * sc, 3.0 * sc), off + vec2(10.2 * sc, 3.0 * sc), off + vec2(10.2 * sc, 21.0 * sc), off + vec2(7.6 * sc, 21.0 * sc)],
                tint,
                Stroke::NONE,
            ));
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(13.8 * sc, 3.0 * sc), off + vec2(17.0 * sc, 3.0 * sc), off + vec2(16.4 * sc, 21.0 * sc), off + vec2(13.8 * sc, 21.0 * sc)],
                tint,
                Stroke::NONE,
            ));
            for (y0, y1) in [(5.0, 8.0), (10.5, 13.5), (16.0, 19.0)] {
                l((12.0, y0), (12.0, y1));
            }
        }
        IconName::Point => {
            p.add(egui::Shape::convex_polygon(
                vec![off + vec2(12.0 * sc, 21.0 * sc), off + vec2(6.0 * sc, 12.0 * sc), off + vec2(7.0 * sc, 7.0 * sc), off + vec2(12.0 * sc, 4.0 * sc), off + vec2(17.0 * sc, 7.0 * sc), off + vec2(18.0 * sc, 12.0 * sc)],
                tint,
                Stroke::NONE,
            ));
            c((12.0, 11.0), 2.2, false);
        }
        IconName::Junction => {
            c((6.0, 6.0), 2.5, false);
            c((6.0, 18.0), 2.5, false);
            c((18.0, 12.0), 2.5, false);
            p.add(egui::Shape::line(pts(&[(8.0, 7.0), (12.0, 10.0), (15.5, 12.0)]).into(), st));
            p.add(egui::Shape::line(pts(&[(8.0, 17.0), (12.0, 14.0), (15.5, 12.0)]).into(), st));
        }
        IconName::TrafficLight => {
            p.rect_stroke(Rect::from_min_size(off + vec2(8.5 * sc, 3.0 * sc), vec2(7.0 * sc, 13.0 * sc)), 1.5 * sc, st);
            c((12.0, 6.2), 1.3, true);
            c((12.0, 9.5), 1.3, true);
            c((12.0, 12.8), 1.3, true);
            l((12.0, 16.0), (12.0, 21.0));
        }
        IconName::Sign => {
            p.rect_stroke(Rect::from_min_size(off + vec2(4.0 * sc, 5.0 * sc), vec2(16.0 * sc, 8.0 * sc)), 1.5 * sc, st);
            l((12.0, 13.0), (12.0, 20.0));
            l((9.0, 9.0), (15.0, 9.0));
        }
        IconName::Bus => {
            p.rect_stroke(Rect::from_min_size(off + vec2(4.0 * sc, 4.0 * sc), vec2(16.0 * sc, 13.0 * sc)), 2.0 * sc, st);
            l((4.0, 12.0), (20.0, 12.0));
            c((8.0, 19.0), 1.6, false);
            c((16.0, 19.0), 1.6, false);
        }
        IconName::Grid => {
            p.rect_stroke(Rect::from_min_size(off + vec2(4.0 * sc, 4.0 * sc), vec2(16.0 * sc, 16.0 * sc)), 1.5 * sc, st);
            l((4.0, 12.0), (20.0, 12.0));
            l((12.0, 4.0), (12.0, 20.0));
        }
        IconName::Circle => {
            c((12.0, 12.0), 8.0, false);
        }
        IconName::Save => {
            p.add(egui::Shape::line(pts(&[(19.5, 19.5), (4.5, 19.5), (4.5, 4.5), (16.0, 4.5), (19.5, 8.0), (19.5, 19.5)]).into(), st));
            p.rect_stroke(Rect::from_min_size(off + vec2(8.0 * sc, 13.0 * sc), vec2(8.0 * sc, 6.5 * sc)), 1.0 * sc, st);
            p.rect_stroke(Rect::from_min_size(off + vec2(8.0 * sc, 4.5 * sc), vec2(7.0 * sc, 4.5 * sc)), 1.0 * sc, st);
        }
        IconName::Download => {
            l((12.0, 4.0), (12.0, 15.0));
            p.add(egui::Shape::line(pts(&[(7.0, 10.5), (12.0, 15.5), (17.0, 10.5)]).into(), st));
            l((4.5, 19.5), (19.5, 19.5));
        }
    }
}

/// An icon widget of `size` points.
pub fn icon(ui: &mut Ui, name: IconName, size: f32, tint: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), egui::Sense::hover());
    paint(ui, name, rect, tint);
    resp
}
