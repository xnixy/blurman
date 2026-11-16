//! What the glass module (ported from shellman, `src/glass`) needs of shellman's theme: the
//! window's mode and accent, and the tint over the glass. Kept to that, so `glass` can stay as
//! close to shellman's as possible.

use egui::Color32;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

pub use crate::app::look::{mix, with_alpha};

/// The colours the glass reads (`text` only in shellman's tests).
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub struct Colors {
    pub light: bool,
    pub accent: Color32,
    /// The page colour, for the solid fill when transparency effects are off.
    pub bg: Color32,
    pub text: Color32,
}

impl Colors {
    pub fn new(light: bool, accent: Color32) -> Self {
        let tint = |grey: u8, amount: f32| mix(Color32::from_gray(grey), accent, amount);
        let (bg, text) = if light {
            (tint(242, 0.05), tint(28, 0.06))
        } else {
            (tint(16, 0.06), tint(240, 0.03))
        };
        Self {
            light,
            accent,
            bg,
            text,
        }
    }
}

static LIGHT: AtomicBool = AtomicBool::new(false);
static ACCENT: AtomicU32 = AtomicU32::new(0x0067C0);

/// Set the mode and accent the glass follows.
pub fn set(light: bool, accent: Color32) {
    LIGHT.store(light, Ordering::Relaxed);
    ACCENT.store(
        (accent.r() as u32) << 16 | (accent.g() as u32) << 8 | accent.b() as u32,
        Ordering::Relaxed,
    );
}

pub fn colors() -> Colors {
    let v = ACCENT.load(Ordering::Relaxed);
    Colors::new(
        LIGHT.load(Ordering::Relaxed),
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8),
    )
}

/// How much of the backdrop shows through, 0 (solid) to 1 (nearly clear): shellman's default.
pub fn glass() -> f32 {
    0.72
}

/// The one tint over the glass, for a mode, accent and glass amount (shellman's `theme::shell`).
pub fn shell(light: bool, accent: Color32, glass: f32) -> Color32 {
    let grey = if light { 238 } else { 10 };
    let alpha = (225.0 + (110.0 - 225.0) * glass.clamp(0.0, 1.0)).round() as u8;
    with_alpha(mix(Color32::from_gray(grey), accent, 0.06), alpha)
}
