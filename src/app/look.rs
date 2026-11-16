//! The app window's look, after shellman's (`shellman/crates/ui/src/theme`): neutrals carry a
//! faint tint of the accent, and the accent only fills or colours something where it stands out
//! from the page, falling back to the text colour otherwise (so a pale yellow or a black accent
//! never makes a state vanish).

use egui::Color32;

/// Linear blend from `a` to `b`. Alpha comes from `a`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

pub fn with_alpha(c: Color32, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), a)
}

fn luminance(c: Color32) -> f32 {
    let ch = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(c.r()) + 0.7152 * ch(c.g()) + 0.0722 * ch(c.b())
}

/// WCAG contrast ratio between two colours, 1 to 21.
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `color` when it stands out from `surface` by at least `min`; otherwise `fallback`.
fn legible(color: Color32, surface: Color32, fallback: Color32, min: f32) -> Color32 {
    if contrast(color, surface) >= min {
        color
    } else {
        fallback
    }
}

/// Least contrast an accent fill needs against the page, and accent text or a glyph.
const FILL_CONTRAST: f32 = 1.6;
const TEXT_CONTRAST: f32 = 3.0;

/// The colours that differ between dark and light, or depend on the accent. The full set, as
/// shellman has it; not every page uses every one yet.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub struct Look {
    pub light: bool,
    /// The accent where it fills something (the current page, primary buttons, a switch that is
    /// on): the accent itself, or the text colour when it would hardly show on the page.
    pub accent_fill: Color32,
    /// Text on a solid accent fill.
    pub on_accent: Color32,
    /// The accent adjusted to read as text or a glyph on the page.
    pub accent_text: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub hairline: Color32,
    /// A switch's or slider's track when off.
    pub track: Color32,
    /// Islands: cards and the sidebar, over the window's glass.
    pub island: Color32,
    pub sidebar: Color32,
    /// Pills: buttons, chips, the header, and their hovered fill.
    pub control: Color32,
    pub control_hover: Color32,
    /// A row's hover and selection.
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub danger: Color32,
    pub danger_text: Color32,
}

impl Look {
    pub fn new(light: bool, accent: Color32) -> Self {
        let tint = |grey: u8, amount: f32| mix(Color32::from_gray(grey), accent, amount);
        let (bg, text, text_base) = if light {
            (
                tint(242, 0.05),
                tint(28, 0.06),
                mix(accent, Color32::BLACK, 0.25),
            )
        } else {
            (
                tint(16, 0.06),
                tint(240, 0.03),
                mix(accent, Color32::WHITE, 0.4),
            )
        };
        let accent_fill = legible(accent, bg, text, FILL_CONTRAST);
        // A selected row is a wash of the accent under ordinary text: of the text colour instead
        // where the accent fell back, or is so light (a yellow on dark) that light text on its
        // wash would fade.
        let fell_back = accent_fill != accent || (!light && luminance(accent_fill) > 0.5);
        let accent_text = legible(text_base, bg, text, TEXT_CONTRAST);
        let on_accent = if luminance(accent_fill) > 0.4 {
            Color32::from_rgb(20, 16, 20)
        } else {
            Color32::WHITE
        };
        if light {
            Self {
                light,
                accent_fill,
                on_accent,
                accent_text,
                text,
                // Darker than shellman's 110: small grey text on Espalier's see-through window
                // renders lighter than its colour.
                muted: tint(84, 0.08),
                hairline: Color32::from_black_alpha(16),
                track: tint(188, 0.06),
                // Nearly opaque: grey text over a see-through light fill reads pale (see
                // `appwin::glass_backing`).
                island: with_alpha(tint(255, 0.02), 235),
                sidebar: with_alpha(tint(238, 0.05), 235),
                control: with_alpha(tint(255, 0.02), 170),
                control_hover: with_alpha(tint(255, 0.03), 235),
                row_hover: Color32::from_black_alpha(14),
                row_selected: if fell_back {
                    with_alpha(text, 22)
                } else {
                    with_alpha(accent_fill, 48)
                },
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(150, 24, 28),
            }
        } else {
            Self {
                light,
                accent_fill,
                on_accent,
                accent_text,
                text,
                muted: tint(150, 0.08),
                hairline: Color32::from_rgba_unmultiplied(255, 255, 255, 16),
                track: tint(66, 0.1),
                island: with_alpha(tint(34, 0.08), 214),
                sidebar: with_alpha(tint(12, 0.06), 190),
                // Deep tinted fills, not translucent white: over glass, white lifts toward grey.
                control: with_alpha(tint(44, 0.08), 210),
                control_hover: with_alpha(tint(56, 0.09), 225),
                row_hover: Color32::from_rgba_unmultiplied(255, 255, 255, 14),
                row_selected: if fell_back {
                    with_alpha(text, 34)
                } else {
                    with_alpha(accent_fill, 64)
                },
                danger: Color32::from_rgb(196, 43, 28),
                danger_text: Color32::from_rgb(255, 160, 164),
            }
        }
    }
}

impl Look {
    /// Windows' high contrast theme: its own colours throughout (window, text, highlight, grey
    /// text, hyperlink), no tints or washes, and a visible line around every island.
    pub fn high_contrast() -> Self {
        use windows::Win32::Graphics::Gdi::{
            GetSysColor, COLOR_GRAYTEXT, COLOR_HIGHLIGHT, COLOR_HIGHLIGHTTEXT, COLOR_HOTLIGHT,
            COLOR_WINDOW, COLOR_WINDOWTEXT,
        };
        let sys = |i| {
            // SAFETY: a plain read of a system colour (0x00BBGGRR).
            let c = unsafe { GetSysColor(i) };
            Color32::from_rgb(
                (c & 0xff) as u8,
                (c >> 8 & 0xff) as u8,
                (c >> 16 & 0xff) as u8,
            )
        };
        let (window, text, highlight, on_highlight, grey, link) = (
            sys(COLOR_WINDOW),
            sys(COLOR_WINDOWTEXT),
            sys(COLOR_HIGHLIGHT),
            sys(COLOR_HIGHLIGHTTEXT),
            sys(COLOR_GRAYTEXT),
            sys(COLOR_HOTLIGHT),
        );
        Self {
            light: luminance(window) > 0.5,
            accent_fill: highlight,
            on_accent: on_highlight,
            accent_text: link,
            text,
            muted: grey,
            hairline: text,
            track: grey,
            island: window,
            sidebar: window,
            control: window,
            control_hover: highlight,
            // No hover wash: ordinary text on a highlight wash isn't a pair Windows allows.
            row_hover: Color32::TRANSPARENT,
            row_selected: highlight,
            danger: text,
            danger_text: text,
        }
    }
}

/// Island and pill geometry, as shellman draws them.
pub const ISLAND_RADIUS: u8 = 22;
pub const PILL_H: f32 = 30.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pale_accent_stays_readable() {
        // A light yellow (as Windows sometimes picks) on a light page falls back to text.
        let l = Look::new(true, Color32::from_rgb(255, 230, 80));
        assert!(contrast(l.accent_fill, Color32::from_gray(242)) >= FILL_CONTRAST);
        assert!(contrast(l.on_accent, l.accent_fill) >= 3.0);
        // On dark it shows, with dark text on it.
        let d = Look::new(false, Color32::from_rgb(255, 230, 80));
        assert_eq!(d.accent_fill, Color32::from_rgb(255, 230, 80));
        assert!(contrast(d.on_accent, d.accent_fill) >= 3.0);
    }
}
