//! The one place every size, offset and radius passes before it reaches Windows.UI.Composition
//! or the backdrop window. Pure, so it is tested with hostile numbers.
//!
//! Why: `dwm.exe` fail-fasts (`dwmcore`, E_INVALIDARG from `CreateTexture2D`) when a visual asks
//! it for a texture of an impossible size: 0, negative, or past 16384. It crashed 16 times on
//! 12 Nov 2026 under glass tests; one path was a piece sized 0 or negative (a window smaller than
//! its shape, or 0x0) whose shadow mask (`CompositionVisualSurface::SetSourceSize`) and host
//! backdrop sprite were sized so while the shadow stayed visible. So here:
//!
//! - a side is 1 to [`MAX_SIDE`] pixels: below 1 the piece is hidden (never sized to 0), above it
//!   is clamped; [`MAX_SIDE`] leaves room for the shadow's spread well under 16384;
//! - offsets are clamped to [`MAX_COORD`], radii to half the shorter side, opacity to 0..1;
//! - the shadow's blur and offset are clamped, a scale that is 0, negative, NaN or infinite
//!   becomes 1;
//! - backdrop windows (each a composition target and its textures) are made at most a few per
//!   second per thread, whatever a caller does ([`Bucket`]).

use super::tree::{Paint, Placed};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

/// The largest side of a piece or the backdrop window, in pixels.
pub const MAX_SIDE: i32 = 8192;
/// The furthest a piece may sit from the backdrop window's corner, in pixels.
pub const MAX_COORD: i32 = 16384;
/// The largest shadow blur radius, in pixels (the theme's is 32 pt: 96 px at 300%).
pub const MAX_BLUR: f32 = 160.0;
/// The largest shadow offset, in pixels.
pub const MAX_OFFSET: f32 = 64.0;
/// Pixels per point outside this are taken as 1.
pub const SCALE: (f32, f32) = (0.25, 8.0);

static CLAMPED: AtomicBool = AtomicBool::new(false);

/// Log once (under `SHELLMAN_GLASS_LOG`) that something was clamped.
fn clamped(what: impl FnOnce() -> String) {
    if !CLAMPED.swap(true, Ordering::Relaxed) {
        super::trace(|| format!("glass: clamped {} (logged once)", what()));
    }
}

/// A usable scale: finite and in [`SCALE`], else 1.
pub fn scale(s: f32) -> f32 {
    if s.is_finite() && (SCALE.0..=SCALE.1).contains(&s) {
        s
    } else {
        1.0
    }
}

/// A finite value clamped to `lo..=hi`; `fallback` when it is NaN or infinite.
pub fn finite(v: f32, lo: f32, hi: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        fallback
    }
}

/// One side: None below 1 (nothing to show), clamped to [`MAX_SIDE`].
pub fn side(v: i32) -> Option<i32> {
    if v < 1 {
        return None;
    }
    if v > MAX_SIDE {
        clamped(|| format!("a side of {v} px to {MAX_SIDE}"));
    }
    Some(v.min(MAX_SIDE))
}

/// A piece as composition may be given it, or None when it must be hidden.
pub fn piece(p: &Placed) -> Option<Placed> {
    let size = (side(p.size.0)?, side(p.size.1)?);
    let half = size.0.min(size.1) as f32 / 2.0;
    Some(Placed {
        left: p.left.clamp(-MAX_COORD, MAX_COORD),
        top: p.top.clamp(-MAX_COORD, MAX_COORD),
        size,
        radius: finite(p.radius, 0.0, half, 0.0),
        opacity: finite(p.opacity, 0.0, 1.0, 1.0),
        shadow: p.shadow,
    })
}

/// The shadow's numbers made sane.
pub fn paint(p: Paint) -> Paint {
    Paint {
        shadow_blur: finite(p.shadow_blur, 0.0, MAX_BLUR, 0.0),
        shadow_offset: finite(p.shadow_offset, -MAX_OFFSET, MAX_OFFSET, 0.0),
        ..p
    }
}

/// The backdrop window's size, or None when it must be hidden.
pub fn window(size: (i32, i32)) -> Option<(i32, i32)> {
    Some((side(size.0)?, side(size.1)?))
}

/// The smallest side a popup glass window (a menu, a preview panel) is ever given, in pixels.
pub const MIN_POPUP: i32 = 24;

/// A popup glass window's size, if it is fit to give the window: each side at least
/// [`MIN_POPUP`] (a smaller one is refused, never shrunk to nothing), and no larger than `work`
/// (the monitor's work area) nor [`MAX_SIDE`].
pub fn popup(size: (i32, i32), work: (i32, i32)) -> Option<(i32, i32)> {
    if size.0 < MIN_POPUP || size.1 < MIN_POPUP {
        return None;
    }
    let cap = |v: i32, lim: i32| v.min(lim.clamp(MIN_POPUP, MAX_SIDE));
    Some((cap(size.0, work.0), cap(size.1, work.1)))
}

/// Keeps changes to a glass window's size at least a gap apart: a caller that would resize on
/// every hover (sliding along a dock) is told to wait, and then applies only the latest size.
#[derive(Clone, Copy, Debug)]
pub struct Pace {
    gap: std::time::Duration,
    last: Option<Instant>,
}

impl Pace {
    pub const fn new(gap: std::time::Duration) -> Self {
        Self { gap, last: None }
    }

    /// May the window change now? `Err(wait)` says how long until it may.
    pub fn take(&mut self, now: Instant) -> Result<(), std::time::Duration> {
        if let Some(last) = self.last {
            let due = last + self.gap;
            if now < due {
                return Err(due - now);
            }
        }
        self.last = Some(now);
        Ok(())
    }
}

/// Allows a burst of [`Bucket::BURST`] and then [`Bucket::PER_SECOND`] a second: how often a
/// thread may make a backdrop window, so a caller that attaches glass per click or per frame
/// cannot make DWM create and destroy targets and textures at that pace.
#[derive(Clone, Copy, Debug)]
pub struct Bucket {
    tokens: f32,
    at: Option<Instant>,
}

impl Default for Bucket {
    fn default() -> Self {
        Self {
            tokens: Self::BURST,
            at: None,
        }
    }
}

impl Bucket {
    pub const BURST: f32 = 8.0;
    pub const PER_SECOND: f32 = 3.0;

    /// Take one at `now`; false when none is left.
    pub fn take(&mut self, now: Instant) -> bool {
        if let Some(at) = self.at {
            let refill = now.saturating_duration_since(at).as_secs_f32() * Self::PER_SECOND;
            self.tokens = (self.tokens + refill).min(Self::BURST);
        }
        self.at = Some(now);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }

    /// How long until one can be taken, from `now` (zero when one can be now). Only looks: it
    /// takes nothing and changes nothing.
    pub fn ready_in(&self, now: Instant) -> std::time::Duration {
        let mut tokens = self.tokens;
        if let Some(at) = self.at {
            tokens = (tokens + now.saturating_duration_since(at).as_secs_f32() * Self::PER_SECOND)
                .min(Self::BURST);
        }
        if tokens >= 1.0 {
            std::time::Duration::ZERO
        } else {
            std::time::Duration::from_secs_f32((1.0 - tokens) / Self::PER_SECOND)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::panel::style::{Geometry, Luminosity, Shadow, Style};
    use egui::Color32;
    use std::time::Duration;

    fn placed(w: i32, h: i32, radius: f32) -> Placed {
        Placed {
            left: 10,
            top: 10,
            size: (w, h),
            radius,
            opacity: 1.0,
            shadow: true,
        }
    }

    fn style(radius: f32, blur: f32, offset: f32) -> Style {
        Style {
            radius,
            luminosity: Luminosity::NONE,
            tint: Color32::BLACK,
            rim: Color32::WHITE,
            shadow: Shadow {
                offset_y: offset,
                blur,
                color: Color32::BLACK,
            },
            solid: Color32::BLACK,
        }
    }

    #[test]
    fn empty_and_negative_pieces_are_hidden_not_sized_to_zero() {
        for (w, h) in [
            (0, 0),
            (0, 50),
            (50, 0),
            (-1, 50),
            (50, -300),
            (i32::MIN, i32::MIN),
        ] {
            assert_eq!(piece(&placed(w, h, 8.0)), None, "{w}x{h}");
        }
        let one = piece(&placed(1, 1, 8.0)).unwrap();
        assert_eq!(one.size, (1, 1));
        assert_eq!(one.radius, 0.5);
    }

    #[test]
    fn huge_pieces_and_windows_are_clamped_well_under_the_texture_limit() {
        let p = piece(&placed(i32::MAX, 100_000, 24.0)).unwrap();
        assert_eq!(p.size, (MAX_SIDE, MAX_SIDE));
        assert_eq!(window((40_000, 1)), Some((MAX_SIDE, 1)));
        assert_eq!(window((0, 900)), None);
        assert_eq!(window((900, -5)), None);
        // Room for the largest shadow on both sides still fits under 16384.
        assert!(MAX_SIDE as f32 + 4.0 * MAX_BLUR + 2.0 * MAX_OFFSET < 16384.0);
    }

    #[test]
    fn radius_and_opacity_are_sane() {
        assert_eq!(piece(&placed(100, 40, 500.0)).unwrap().radius, 20.0);
        assert_eq!(piece(&placed(100, 40, -3.0)).unwrap().radius, 0.0);
        assert_eq!(piece(&placed(100, 40, f32::NAN)).unwrap().radius, 0.0);
        assert_eq!(piece(&placed(100, 40, f32::INFINITY)).unwrap().radius, 0.0);
        let mut p = placed(10, 10, 2.0);
        p.opacity = f32::NAN;
        assert_eq!(piece(&p).unwrap().opacity, 1.0);
        p.opacity = 7.0;
        assert_eq!(piece(&p).unwrap().opacity, 1.0);
        p.opacity = -1.0;
        assert_eq!(piece(&p).unwrap().opacity, 0.0);
    }

    #[test]
    fn offsets_stay_in_range() {
        let mut p = placed(10, 10, 2.0);
        p.left = i32::MIN;
        p.top = i32::MAX;
        let q = piece(&p).unwrap();
        assert_eq!((q.left, q.top), (-MAX_COORD, MAX_COORD));
    }

    #[test]
    fn shadow_numbers_are_clamped() {
        let s = style(8.0, 32.0, 8.0);
        let raw = |blur, offset| Paint {
            style: s,
            effects: true,
            shadow_blur: blur,
            shadow_offset: offset,
        };
        let p = paint(raw(f32::NAN, f32::INFINITY));
        assert_eq!((p.shadow_blur, p.shadow_offset), (0.0, 0.0));
        let p = paint(raw(1e9, -1e9));
        assert_eq!((p.shadow_blur, p.shadow_offset), (MAX_BLUR, -MAX_OFFSET));
        let p = paint(raw(-5.0, 12.0));
        assert_eq!((p.shadow_blur, p.shadow_offset), (0.0, 12.0));
    }

    #[test]
    fn a_bad_scale_is_taken_as_one() {
        for s in [
            0.0,
            -1.0,
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            1e-9,
            1e9,
        ] {
            assert_eq!(scale(s), 1.0, "{s}");
        }
        assert_eq!(scale(1.5), 1.5);
        assert_eq!(scale(3.0), 3.0);
    }

    #[test]
    fn geometry_survives_hostile_input() {
        for scale in [0.0, f32::NAN, f32::INFINITY, -2.0, 1.0, 3.0] {
            for s in [
                style(24.0, 32.0, 8.0),
                style(f32::NAN, f32::NAN, f32::NAN),
                style(1e9, 1e9, -1e9),
                style(-5.0, -5.0, 1e9),
            ] {
                for shape in [(0, 0), (-50, 20), (1, 1), (100, 40), (i32::MAX / 2, 10)] {
                    let g = Geometry::new(shape, scale, &s);
                    assert!(g.radius.is_finite() && g.radius >= 0.0);
                    assert!(g.shadow_blur.is_finite() && g.shadow_blur <= MAX_BLUR);
                    assert!(g.shadow_offset.is_finite() && g.shadow_offset.abs() <= MAX_OFFSET);
                    assert!(g.left >= 0 && g.top >= 0 && g.right >= 0 && g.bottom >= 0);
                    // The room around the shape stays small, whatever came in.
                    assert!(g.left + g.right + g.top + g.bottom < 2048);
                    if let Some(p) = piece(&placed(g.shape.0, g.shape.1, g.radius)) {
                        assert!(p.radius <= p.size.0.min(p.size.1) as f32 / 2.0);
                    }
                }
            }
        }
    }

    #[test]
    fn a_window_smaller_than_its_shadow_room_gives_no_piece() {
        // A 0x0 or tiny window: the shape is what is left inside the room, nothing.
        let g = Geometry::new((0, 0), 3.0, &style(24.0, 32.0, 8.0));
        assert_eq!(piece(&placed(g.shape.0, g.shape.1, g.radius)), None);
        // The backdrop window is then only the shadow's room: valid, but showing no piece.
        assert!(window(g.window()).is_some());
    }

    #[test]
    fn popups_are_never_tiny_nor_past_the_work_area() {
        assert_eq!(popup((400, 200), (3840, 2000)), Some((400, 200)));
        assert_eq!(popup((0, 200), (3840, 2000)), None);
        assert_eq!(popup((400, -5), (3840, 2000)), None);
        assert_eq!(popup((23, 400), (3840, 2000)), None);
        assert_eq!(popup((9000, 3000), (3840, 2000)), Some((3840, 2000)));
        assert_eq!(
            popup((i32::MAX, i32::MAX), (100_000, 100_000)),
            Some((MAX_SIDE, MAX_SIDE))
        );
        // A nonsense work area still gives a sane window.
        assert_eq!(popup((400, 200), (0, -1)), Some((MIN_POPUP, MIN_POPUP)));
    }

    #[test]
    fn pace_spaces_changes_out() {
        let t = Instant::now();
        let mut p = Pace::new(Duration::from_millis(50));
        assert!(p.take(t).is_ok());
        assert_eq!(
            p.take(t + Duration::from_millis(20)),
            Err(Duration::from_millis(30))
        );
        assert!(p.take(t + Duration::from_millis(50)).is_ok());
        assert!(p.take(t + Duration::from_millis(60)).is_err());
    }

    #[test]
    fn the_bucket_allows_a_burst_then_a_few_a_second() {
        let t0 = Instant::now();
        let mut b = Bucket::default();
        let burst = (0..100).filter(|_| b.take(t0)).count();
        assert_eq!(burst, Bucket::BURST as usize);
        // A caller trying every frame (60 a second) for 10 seconds gets ~3 a second.
        let mut got = 0;
        for frame in 1..=600 {
            if b.take(t0 + Duration::from_millis(frame * 1000 / 60)) {
                got += 1;
            }
        }
        assert!((28..=31).contains(&got), "{got}");
        // After a quiet spell the burst is back, never more.
        let later = t0 + Duration::from_secs(60);
        let burst = (0..100).filter(|_| b.take(later)).count();
        assert_eq!(burst, Bucket::BURST as usize);
    }
}
