//! tsui — the yakui-style UI layer for traffic-sim.
//!
//! A lib copy of the egregoria HUD layout (goryak, MIT): TOP-LEFT pinned
//! toolbox with a per-tool properties strip (blur panel) above a row of
//! 64×64 icon buttons, selection triangle, and a bottom status bar — drawn
//! over the software-rasterized frame. Text uses an embedded 5×7 bitmap
//! font (public-domain glcdfont-style columns), icons are the copied
//! egregoria `assets/ui/icons/toolbar_*.png` files blitted with alpha.

use image::GenericImageView;

/// one glyph = 5 columns × 7 rows (column bytes, LSB = top row)
fn glyph(c: char) -> Option<&'static [u8; 5]> {
    Some(match c {
        'A' => b"\x7E\x09\x09\x09\x7E",
        'B' => b"\x7F\x49\x49\x49\x36",
        'C' => b"\x3E\x41\x41\x41\x22",
        'D' => b"\x7F\x41\x41\x22\x1C",
        'E' => b"\x7F\x49\x49\x49\x41",
        'F' => b"\x7F\x09\x09\x01\x01",
        'G' => b"\x3E\x41\x49\x49\x3A",
        'H' => b"\x7F\x08\x08\x08\x7F",
        'I' => b"\x00\x41\x7F\x41\x00",
        'J' => b"\x20\x40\x41\x3F\x01",
        'K' => b"\x7F\x08\x14\x22\x41",
        'L' => b"\x7F\x40\x40\x40\x40",
        'M' => b"\x7F\x02\x04\x02\x7F",
        'N' => b"\x7F\x04\x08\x10\x7F",
        'O' => b"\x3E\x41\x41\x41\x3E",
        'P' => b"\x7F\x09\x09\x09\x06",
        'Q' => b"\x3E\x41\x51\x21\x5E",
        'R' => b"\x7F\x09\x19\x29\x46",
        'S' => b"\x26\x49\x49\x49\x32",
        'T' => b"\x01\x01\x7F\x01\x01",
        'U' => b"\x3F\x40\x40\x40\x3F",
        'V' => b"\x1F\x20\x40\x20\x1F",
        'W' => b"\x7F\x20\x18\x20\x7F",
        'X' => b"\x63\x14\x08\x14\x63",
        'Y' => b"\x03\x04\x78\x04\x03",
        'Z' => b"\x61\x51\x49\x45\x43",
        '0' => b"\x3E\x51\x49\x45\x3E",
        '1' => b"\x00\x42\x7F\x40\x00",
        '2' => b"\x42\x61\x51\x49\x46",
        '3' => b"\x21\x41\x45\x4B\x31",
        '4' => b"\x18\x14\x12\x7F\x10",
        '5' => b"\x27\x45\x45\x45\x39",
        '6' => b"\x3C\x4A\x49\x49\x30",
        '7' => b"\x01\x71\x09\x05\x03",
        '8' => b"\x36\x49\x49\x49\x36",
        '9' => b"\x06\x49\x49\x29\x1E",
        ' ' => b"\x00\x00\x00\x00\x00",
        '.' => b"\x00\x60\x60\x00\x00",
        ',' => b"\x00\x50\x30\x00\x00",
        ':' => b"\x00\x36\x36\x00\x00",
        '-' => b"\x08\x08\x08\x08\x08",
        '_' => b"\x80\x80\x80\x80\x80",
        '/' => b"\x20\x10\x08\x04\x02",
        '%' => b"\x63\x14\x18\x14\x63",
        '(' => b"\x00\x22\x41\x41\x00",
        ')' => b"\x00\x41\x41\x22\x00",
        '+' => b"\x08\x08\x3E\x08\x08",
        '*' => b"\x2A\x1C\x7F\x1C\x2A",
        '=' => b"\x14\x14\x14\x14\x14",
        '>' => b"\x41\x22\x14\x08\x00",
        '<' => b"\x00\x08\x14\x22\x41",
        '[' => b"\x00\x7E\x41\x41\x00",
        ']' => b"\x00\x41\x41\x7E\x00",
        _ => return None,
    })
}

/// RGBA painter over an RGBA8 buffer.
pub struct Painter<'a> {
    pub buf: &'a mut [u8],
    pub w: usize,
    pub h: usize,
}

impl<'a> Painter<'a> {
    pub fn new(buf: &'a mut [u8], w: usize, h: usize) -> Self {
        Painter { buf, w, h }
    }

    fn blend(&mut self, x: usize, y: usize, c: [f32; 4]) {
        if x >= self.w || y >= self.h {
            return;
        }
        let i = (y * self.w + x) * 4;
        let a = c[3].clamp(0.0, 1.0);
        for k in 0..3 {
            let d = self.buf[i + k] as f32 / 255.0;
            self.buf[i + k] = ((c[k] * a + d * (1.0 - a)) * 255.0) as u8;
        }
        let da = (c[3] + self.buf[i + 3] as f32 / 255.0 * (1.0 - a)).min(1.0);
        self.buf[i + 3] = (da * 255.0) as u8;
    }

    pub fn rect(&mut self, x: i32, y: i32, w: i32, h: i32, c: [f32; 4]) {
        for yy in y.max(0)..(y + h).min(self.h as i32) {
            for xx in x.max(0)..(x + w).min(self.w as i32) {
                self.blend(xx as usize, yy as usize, c);
            }
        }
    }

    /// blur panel = goryak `blur_bg`: dim rounded panel with a bright hairline
    pub fn panel(&mut self, x: i32, y: i32, w: i32, h: i32, c: [f32; 4]) {
        let r = 6;
        self.rect(x + r / 2, y, w - r, h, c);
        self.rect(x, y + r / 2, w, h - r, c);
        let line = [c[0] * 1.4, c[1] * 1.4, c[2] * 1.4, (c[3] + 0.2).min(1.0)];
        for (lx, ly, lw, lh) in [
            (x + r / 2, y, w - r, 1),
            (x + r / 2, y + h - 1, w - r, 1),
            (x, y + r / 2, 1, h - r),
            (x + w - 1, y + r / 2, 1, h - r),
        ] {
            self.rect(lx, ly, lw, lh, line);
        }
    }

    /// selection triangle (goryak select_triangle)
    pub fn triangle(&mut self, x: i32, y: i32, s: i32, c: [f32; 4]) {
        for row in 0..s {
            let w = s - row;
            self.rect(x + row, y + row, w, 1, c);
        }
    }

    /// bitmap text, scale s (1 = 5x7 px). Returns advance in px.
    pub fn text(&mut self, x: i32, y: i32, s: usize, txt: &str, c: [f32; 4]) -> i32 {
        let mut cx = x;
        for ch in txt.chars() {
            if let Some(g) = glyph(ch.to_ascii_uppercase()) {
                for col in 0..5 {
                    let bits = g[col];
                    for row in 0..7 {
                        if bits & (1 << row) != 0 {
                            self.rect(cx + (col * s) as i32, y + (row * s) as i32, s as i32, s as i32, c);
                        }
                    }
                }
            }
            cx += (6 * s) as i32;
        }
        cx - x
    }

    /// blit a PNG (RGBA) with alpha, scaled 1..4, from bytes
    pub fn blit_png(&mut self, png: &[u8], x: i32, y: i32, size: i32) {
        let Ok(img) = image::load_from_memory(png) else { return };
        let (iw, ih) = (img.width() as i32, img.height() as i32);
        if iw == 0 || ih == 0 {
            return;
        }
        let rgba = img.to_rgba8();
        let s = (size as f32 / iw.max(ih) as f32).floor().max(0.2) as i32;
        for yy in 0..ih {
            for xx in 0..iw {
                let px = rgba.get_pixel(xx as u32, yy as u32);
                let a = px.0[3] as f32 / 255.0;
                if a < 0.02 {
                    continue;
                }
                let c = [
                    px.0[0] as f32 / 255.0,
                    px.0[1] as f32 / 255.0,
                    px.0[2] as f32 / 255.0,
                    a,
                ];
                for dy in 0..s {
                    for dx in 0..s {
                        self.blend((x + xx * s + dx) as usize, (y + yy * s + dy) as usize, c);
                    }
                }
            }
        }
    }
}

/// colors — Material-3-ish tokens matching goryak's theme
pub const PRIMARY: [f32; 4] = [0.20, 0.79, 0.57, 1.0];
pub const ON_PRIMARY: [f32; 4] = [0.05, 0.12, 0.09, 1.0];
pub const SURFACE: [f32; 4] = [0.11, 0.12, 0.14, 0.78];
pub const SURFACE_2: [f32; 4] = [0.15, 0.16, 0.18, 0.85];
pub const TEXT: [f32; 4] = [0.95, 0.96, 0.97, 1.0];
pub const DIM: [f32; 4] = [0.72, 0.74, 0.76, 1.0];
pub const DANGER: [f32; 4] = [0.86, 0.30, 0.25, 1.0];

/// toolbox layout constants copied from egregoria's HUD
pub const ICON_BTN: i32 = 64;
pub const PAD: i32 = 6;

/// The hardcoded land-toolbox HUD (egregoria `new_toolbox` layout):
/// TOP-LEFT pinned, tool-properties strip above the 64 px icon row.
#[allow(clippy::too_many_arguments)]
pub fn draw_toolbox(
    p: &mut Painter,
    icons: &[(String, Vec<u8>)], // (name, png bytes) in TOOLS order
    selected: usize,
    tool_name: &str,
    props_lines: &[String],
    clock: &str,
    title: &str,
) {
    let icon = ICON_BTN;
    let n = icons.len() as i32;
    // tools list panel
    let lw = n * (icon + PAD) + PAD;
    let ly = 8;
    p.panel(8, ly, lw, icon + PAD * 2, SURFACE);
    for (i, (_, png)) in icons.iter().enumerate() {
        let bx = 8 + PAD + i as i32 * (icon + PAD);
        let by = ly + PAD;
        let sel = i == selected;
        let bgc = if sel { SURFACE_2 } else { SURFACE };
        p.rect(bx, by, icon, icon, bgc);
        p.blit_png(png, bx + 6, by + 6, icon - 12);
        if sel {
            p.triangle(bx + icon / 2 - 6, by + icon + 2, 12, PRIMARY);
            p.rect(bx, by, icon, 2, PRIMARY);
        }
    }
    // tool properties strip (blur panel above the list)
    let ph = (props_lines.len() as i32 + 1) * 14 + 12;
    p.panel(8, ly + icon + PAD * 2 + 6, (lw).max(300), ph, SURFACE_2);
    let mut ty = ly + icon + PAD * 2 + 12;
    p.text(8 + PAD, ty, 1, &format!("{} {}", tool_name.to_uppercase(), clock), PRIMARY);
    ty += 14;
    for line in props_lines {
        p.text(8 + PAD, ty, 1, line, DIM);
        ty += 14;
    }
    // app title, top-right (yakui menu_bar substitute)
    let tw = title.len() as i32 * 6 + 16;
    p.panel(p.w as i32 - tw - 8, 8, tw, 22, SURFACE);
    p.text(p.w as i32 - tw - 8 + 8, 14, 1, title, TEXT);
}

/// bottom status bar (egregoria statusbar, 24 px)
pub fn draw_status(p: &mut Painter, left: &str, right: &str) {
    let h = 24;
    p.rect(0, p.h as i32 - h, p.w as i32, h, [0.08, 0.09, 0.11, 0.92]);
    p.rect(0, p.h as i32 - h, p.w as i32, 1, [0.3, 0.9, 0.65, 0.5]);
    p.text(8, p.h as i32 - h + 8, 1, left, DIM);
    let rw = right.len() as i32 * 6 + 8;
    p.text(p.w as i32 - rw, p.h as i32 - h + 8, 1, right, DIM);
}
