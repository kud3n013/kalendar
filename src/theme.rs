//! How things look: the palette, the colours that mean something, the text
//! sizes and the corner radii. Drawing code names a role from here instead
//! of writing a number, so the same thing looks the same everywhere.

use eframe::egui::{self, Color32, FontId};

// ---- colours that mean the same in light and dark ----

/// the app's own colour: selection, the current choice
pub const ACCENT: Color32 = Color32::from_rgb(0x62, 0x5e, 0xfc);
/// deleting, removing, errors, overdue
pub const DANGER: Color32 = Color32::from_rgb(0xeb, 0x62, 0x62);
/// connected, working
pub const SUCCESS: Color32 = Color32::from_rgb(0x4c, 0xc3, 0x7a);
/// due today or soon
pub const WARNING: Color32 = Color32::from_rgb(0xff, 0x8f, 0x1e);
/// the line at the current time
pub const NOW: Color32 = Color32::from_rgb(0xea, 0x43, 0x35);
/// dark text on a light block
pub const INK: Color32 = Color32::from_rgb(0x10, 0x20, 0x26);

/// The colours that change with light and dark.
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub side: Color32,
    pub rail: Color32,
    pub border: Color32,
    pub border_minor: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub today: Color32,
    pub weekend: Color32,
    pub now: Color32,
}

impl Palette {
    pub fn new(dark: bool) -> Self {
        if dark {
            Palette {
                dark,
                bg: Color32::from_rgb(0x22, 0x22, 0x22),
                side: Color32::from_rgb(0x1c, 0x1c, 0x1c),
                rail: Color32::from_rgb(0x1c, 0x1c, 0x1c),
                border: Color32::from_rgb(0x3a, 0x3a, 0x3a),
                border_minor: Color32::from_rgb(0x30, 0x30, 0x30),
                text: Color32::from_rgb(0xf4, 0xf4, 0xf5),
                muted: Color32::from_rgb(0x96, 0x95, 0xa0),
                accent: ACCENT,
                today: Color32::from_rgb(0x2a, 0x2d, 0x35),
                weekend: Color32::from_rgb(0x29, 0x29, 0x29),
                now: NOW,
            }
        } else {
            Palette {
                dark,
                bg: Color32::WHITE,
                side: Color32::WHITE,
                rail: Color32::from_rgb(0xf7, 0xf7, 0xf7),
                border: Color32::from_rgb(0xe1, 0xe0, 0xe0),
                border_minor: Color32::from_rgb(0xec, 0xec, 0xec),
                text: Color32::from_rgb(0x1f, 0x23, 0x28),
                muted: Color32::from_rgb(0x7f, 0x82, 0x85),
                accent: ACCENT,
                today: Color32::from_rgb(0xf3, 0xf7, 0xfe),
                weekend: Color32::from_rgb(0xf9, 0xf9, 0xf9),
                now: NOW,
            }
        }
    }
}

/// `a` moved towards `b` by `t` (0 to 1).
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

// ---- text ----

/// Text sizes, smallest first.
pub mod size {
    /// key hints squeezed beside a label
    pub const MINI: f32 = 10.0;
    /// small print on blocks and chips
    pub const MICRO: f32 = 11.0;
    pub const TINY: f32 = 11.5;
    pub const SMALL: f32 = 12.0;
    /// secondary lines, notes under a setting
    pub const CAPTION: f32 = 13.0;
    /// most text in rows and menus
    pub const BODY: f32 = 14.0;
    pub const UI: f32 = 15.0;
    pub const LARGE: f32 = 16.0;
    /// a setting's name
    pub const TITLE: f32 = 17.0;
    pub const HEADING: f32 = 18.0;
}

/// The bold face at `size`.
pub fn bold(size: f32) -> FontId {
    FontId::new(size, egui::FontFamily::Name("bold".into()))
}

// ---- shapes ----

/// the corner of blocks, popups and most frames
pub const RADIUS: f32 = 8.0;
/// buttons drawn by hand
pub const BUTTON_RADIUS: f32 = 7.0;
/// cards and toasts
pub const CARD_RADIUS: f32 = 10.0;
/// small chips, such as key caps
pub const CHIP_RADIUS: f32 = 5.0;

/// The outline of a squircle: a rectangle whose corners are superellipse
/// arcs (continuous curvature) instead of circular ones.
pub fn squircle_points(rect: egui::Rect, radius: f32) -> Vec<egui::Pos2> {
    let r = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
    if r <= 0.5 {
        return vec![rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];
    }
    const STEPS: usize = 10;
    // |x|^4 + |y|^4 = r^4, so the offsets are cos(t)^0.5 and sin(t)^0.5
    let arc = |i: usize| {
        let t = std::f32::consts::FRAC_PI_2 * i as f32 / STEPS as f32;
        (t.cos().max(0.0).sqrt(), t.sin().max(0.0).sqrt())
    };
    let mut pts = Vec::with_capacity(4 * (STEPS + 1));
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(rt - r + r * s, t + r - r * c)); // top right
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(rt - r + r * c, b - r + r * s)); // bottom right
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(l + r - r * s, b - r + r * c)); // bottom left
    }
    for i in 0..=STEPS {
        let (c, s) = arc(i);
        pts.push(egui::pos2(l + r - r * c, t + r - r * s)); // top left
    }
    pts
}

pub fn fill_squircle(p: &egui::Painter, rect: egui::Rect, radius: f32, fill: Color32) {
    p.add(egui::Shape::convex_polygon(squircle_points(rect, radius), fill, egui::Stroke::NONE));
}

pub fn stroke_squircle(p: &egui::Painter, rect: egui::Rect, radius: f32, stroke: egui::Stroke) {
    p.add(egui::Shape::closed_line(squircle_points(rect, radius), stroke));
}
