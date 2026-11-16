//! How things move. Every duration and easing lives here, so the same kind
//! of motion runs at the same speed wherever it happens: a page turned by a
//! key, a button or a block carried to the edge all slide as one.

use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::epaint::ClippedShape;

/// Whether things move at all; follows the setting under Appearance.
pub static ANIMATE: AtomicBool = AtomicBool::new(true);

/// How long one kind of motion takes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speed(pub f32);

impl Speed {
    /// The time in seconds, or nothing while animations are off.
    pub fn secs(self) -> f32 {
        if on() {
            self.0
        } else {
            0.0
        }
    }
}

/// glows under the pointer, switches flipping
pub const HOVER: Speed = Speed(0.1);
/// a block catching up with the pointer that drags it
pub const FOLLOW: Speed = Speed(0.06);
/// opening, folding, fading in and out, blocks settling into place
pub const NORMAL: Speed = Speed(0.175);
/// a window changing its size, another view fading in
pub const LONG: Speed = Speed(0.275);
/// one page of the calendar sliding over to the next
pub const SLIDE: Speed = Speed(0.55);

/// how long a dragged block waits at the edge before the page turns
pub const EDGE_HOLD_S: f64 = 0.3;
/// and the pause between turns while it stays there
pub const EDGE_REPEAT_S: f64 = 0.25;

/// how quickly the hours glide to a new zoom, per second
pub const ZOOM_RATE: f32 = 14.0;

/// Whether anything moves at all.
pub fn on() -> bool {
    ANIMATE.load(Ordering::Relaxed)
}

/// Gentle at both ends (cubic in-out), for `t` from 0 to 1.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// The calendar page moving in time. The page on screen slides from an
/// offset back to rest; the pages it pushes aside are kept as they were
/// last painted, each at its resting place beside the current one. A new
/// step while it moves starts from where things are, so going back half way
/// turns the slide round instead of jumping.
pub struct Slide {
    began: f64,
    /// where the page on screen started, in points from its resting place
    from: f32,
    /// a fade, for a change of view, in place of a slide
    fade: bool,
    /// the pages slid aside, with their resting places beside this one
    pub pages: Vec<(Vec<ClippedShape>, f32)>,
}

impl Default for Slide {
    fn default() -> Self {
        Self { began: f64::NEG_INFINITY, from: 0.0, fade: false, pages: vec![] }
    }
}

impl Slide {
    fn t(&self, now: f64) -> f32 {
        let secs = if self.fade { LONG } else { SLIDE }.secs();
        if secs <= 0.0 {
            1.0
        } else {
            (((now - self.began) as f32) / secs).clamp(0.0, 1.0)
        }
    }

    pub fn moving(&self, now: f64) -> bool {
        self.t(now) < 1.0
    }

    pub fn sliding(&self, now: f64) -> bool {
        !self.fade && self.moving(now)
    }

    /// How far the page on screen is from its resting place.
    pub fn offset(&self, now: f64) -> f32 {
        if self.fade {
            0.0
        } else {
            self.from * (1.0 - ease_in_out(self.t(now)))
        }
    }

    pub fn opacity(&self, now: f64) -> f32 {
        if self.fade {
            0.25 + 0.75 * ease_in_out(self.t(now))
        } else {
            1.0
        }
    }

    /// The page moved `by` points (forward is positive: the new page comes
    /// in from the right). `last` is the page that was on screen, painted
    /// at rest; `width` is how wide the pages are.
    pub fn step(&mut self, now: f64, by: f32, last: Vec<ClippedShape>, width: f32) {
        let here = if self.sliding(now) { self.offset(now) } else { 0.0 };
        if !self.sliding(now) {
            self.pages.clear();
        }
        for p in &mut self.pages {
            p.1 -= by;
        }
        self.pages.push((last, -by));
        // pressed many times over, it catches up rather than run far behind
        let from = (here + by).clamp(-3.0 * width, 3.0 * width);
        // keep only the pages that will be in view on the way
        self.pages.retain(|p| from.max(0.0) + p.1 > -width && from.min(0.0) + p.1 < width);
        *self = Self { began: now, from, fade: false, pages: std::mem::take(&mut self.pages) };
    }

    /// Another view: no slide, it fades in.
    pub fn fade(&mut self, now: f64) {
        *self = Self { began: now, from: 0.0, fade: true, pages: vec![] };
    }

    /// Once at rest, the pages slid aside are not needed.
    pub fn settle(&mut self, now: f64) {
        if !self.moving(now) {
            self.pages.clear();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn going_back_half_way_turns_round_where_it_is() {
        let w = 100.0;
        let mut s = Slide::default();
        s.step(0.0, w, vec![], w);
        assert_eq!(s.offset(0.0), w);
        let half = SLIDE.0 as f64 / 2.0;
        let here = s.offset(half);
        assert!(here > 0.0 && here < w);
        // back: the page that was arriving is pushed aside from where it is
        s.step(half, -w, vec![], w);
        let arriving = s.pages.last().unwrap().1;
        assert!((s.offset(half) + arriving - here).abs() < 1e-3);
        assert!(s.offset(half) < 0.0);
        assert!(!s.moving(half + SLIDE.0 as f64 + 0.01));
    }

    #[test]
    fn eases_from_zero_to_one() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert_eq!(ease_in_out(1.0), 1.0);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-6);
    }
}
