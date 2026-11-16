//! What a glass surface looks like (in points and colours), and where its parts land in pixels.
//! Pure, so it is tested without a window.

use crate::app::glass_theme::{self as theme, colors, with_alpha, Colors};
use egui::Color32;

/// The soft shadow under a surface.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Shadow {
    /// How far down the shadow falls, in points.
    pub offset_y: f32,
    /// How far it spreads, in points.
    pub blur: f32,
    pub color: Color32,
}

/// What the glass does to the blurred backdrop before the tint goes over it, the way Windows'
/// acrylic does: brightness is squeezed into a band from `lift` to `lift + keep`, and colour is
/// kept. So glass over a black wallpaper is lifted to a grey instead of going black, glass over a
/// white window is held down to where its text stays legible, and a coloured backdrop still shows
/// its colour.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Luminosity {
    /// What black is lifted to, per channel (red, green, blue), 0 to 1.
    pub lift: [f32; 3],
    /// How much of the backdrop's own brightness is kept on top of that, 0 to 1.
    pub keep: f32,
    /// How much of the backdrop's colour is kept (each channel's difference from the
    /// brightness), 0 to 1. More than `keep` makes it more saturated than the brightness alone
    /// would leave it.
    pub colour: f32,
}

/// The weights of red, green and blue in brightness (Rec. 709).
const LUMA: [f32; 3] = [0.2126, 0.7152, 0.0722];

impl Luminosity {
    /// Leaves the backdrop as it is.
    pub const NONE: Self = Self {
        lift: [0.0; 3],
        keep: 1.0,
        colour: 1.0,
    };

    /// What Windows' acrylic (the transient-window backdrop the Files window uses) does, worked
    /// out through Files' sidebar over solid windows of known colours. Dark: black, grey and
    /// white show as 54, 100 and 146 of 255, a straight line, and a blue keeps all of its colour.
    /// Light: the same squeeze from 137, with most of the colour kept.
    pub fn acrylic(light: bool) -> Self {
        if light {
            Self {
                lift: [137.0 / 255.0; 3],
                keep: 0.36,
                colour: 0.75,
            }
        } else {
            Self {
                lift: [54.0 / 255.0; 3],
                keep: 0.36,
                colour: 1.0,
            }
        }
    }

    /// A colour (0 to 1 per channel) as the glass shows it, clamped.
    pub fn apply(&self, rgb: [f32; 3]) -> [f32; 3] {
        let y: f32 = rgb.iter().zip(LUMA).map(|(c, w)| c * w).sum();
        std::array::from_fn(|i| {
            (self.lift[i] + self.keep * y + self.colour * (rgb[i] - y)).clamp(0.0, 1.0)
        })
    }

    /// The same as a Direct2D 5x4 colour matrix (rows: red, green, blue, alpha in, then the
    /// offset; columns: red, green, blue, alpha out), but for the clamp, which the effect does.
    pub fn matrix(&self) -> [f32; 20] {
        let mut m = [0.0; 20];
        for (i, w) in LUMA.iter().enumerate() {
            for j in 0..3 {
                let own = if i == j { self.colour } else { 0.0 };
                m[i * 4 + j] = own + (self.keep - self.colour) * w;
            }
        }
        m[3 * 4 + 3] = 1.0;
        for j in 0..3 {
            m[4 * 4 + j] = self.lift[j];
        }
        m
    }
}

/// The look of one glass surface.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Style {
    /// Corner radius, in points.
    pub radius: f32,
    /// What happens to the blurred backdrop's colours before the tint.
    pub luminosity: Luminosity,
    /// Laid over the blurred backdrop. Its alpha is how much it hides; the user's glass amount
    /// sets it.
    pub tint: Color32,
    /// The 1 px line just inside the edge, at the top; it fades to a third of this lower down.
    pub rim: Color32,
    pub shadow: Shadow,
    /// What fills the shape when Windows has transparency effects turned off.
    pub solid: Color32,
}

impl Style {
    /// The theme's floating glass, in its current mode, accent and glass amount.
    pub fn from_theme(radius: f32) -> Self {
        Self::new(&colors(), theme::glass(), radius)
    }

    /// Floating glass for a palette and a glass amount (0 solid, 1 nearly clear).
    ///
    /// The same material as a glass window (Files, Settings): Windows' acrylic, then the
    /// window's one tint (`theme::shell`) over it, so both hide as much at every glass setting.
    /// [`Luminosity::acrylic`] stands in for the acrylic.
    ///
    /// The window's tint reaches the screen the way egui hands it over: premultiplied in linear
    /// light, then laid over the acrylic by Windows in sRGB. A dark tint shows as its value, but a
    /// light one shows lighter (224 at 43% adds what 355 would). The glass's tint is that same
    /// premultiplied colour; what a colour cannot carry past white goes into the lift instead.
    pub fn new(c: &Colors, glass: f32, radius: f32) -> Self {
        let shell = theme::shell(c.light, c.accent, glass);
        let (tint, extra) = as_window_shows(shell);
        let mut luminosity = Luminosity::acrylic(c.light);
        for (lift, extra) in luminosity.lift.iter_mut().zip(extra) {
            *lift += extra;
        }
        let (rim, shadow) = if c.light {
            (
                Color32::from_white_alpha(150),
                Color32::from_black_alpha(46),
            )
        } else {
            (Color32::from_white_alpha(34), Color32::from_black_alpha(90))
        };
        Self {
            radius,
            luminosity,
            tint,
            rim,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 32.0,
                color: shadow,
            },
            solid: c.bg,
        }
    }
}

/// A window's `tint` as it is seen over its backdrop (see [`Style::new`]): the straight colour
/// that, laid over in sRGB, adds as much as the tint premultiplied in linear light does, and
/// per channel (0 to 1) what of that a colour cannot carry past white, to add to the backdrop
/// under it instead.
fn as_window_shows(tint: Color32) -> (Color32, [f32; 3]) {
    let [r, g, b, a] = tint.to_srgba_unmultiplied();
    let alpha = a as f32 / 255.0;
    if alpha <= 0.0 || alpha >= 1.0 {
        return (tint, [0.0; 3]);
    }
    let parts = [r, g, b].map(|v| {
        let premultiplied = egui::ecolor::gamma_from_linear(
            egui::ecolor::linear_from_gamma(v as f32 / 255.0) * alpha,
        );
        let straight = (premultiplied / alpha).min(1.0);
        let extra = (premultiplied - straight * alpha) / (1.0 - alpha);
        ((straight * 255.0).round() as u8, extra)
    });
    (
        Color32::from_rgba_unmultiplied(parts[0].0, parts[1].0, parts[2].0, a),
        parts.map(|(_, extra)| extra),
    )
}

/// A surface's parts in physical pixels, for a shape of a given pixel size at a given scale.
///
/// The backdrop window is the shape plus room for the shadow on every side; the shape sits at
/// (`left`, `top`) inside it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Geometry {
    pub shape: (i32, i32),
    pub radius: f32,
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub shadow_offset: f32,
    pub shadow_blur: f32,
}

impl Geometry {
    pub fn new(shape: (i32, i32), scale: f32, style: &Style) -> Self {
        use super::guard::{finite, MAX_BLUR, MAX_OFFSET};
        // A scale of 0 or NaN (from DPI maths) would make every number below NaN.
        let scale = super::guard::scale(scale);
        let px = |pt: f32| pt * scale;
        // The radius can never be more than half the shorter side.
        let half = (shape.0.min(shape.1) as f32 / 2.0).max(0.0);
        let radius = finite(px(style.radius), 0.0, half, 0.0);
        let blur = finite(px(style.shadow.blur), 0.0, MAX_BLUR, 0.0);
        let offset = finite(px(style.shadow.offset_y), -MAX_OFFSET, MAX_OFFSET, 0.0);
        // A composition shadow is still visible one blur radius past its shape (seen at 300%,
        // where the window's edge cut it off); it is gone by two.
        let spread = (blur * 2.0).ceil() as i32;
        Self {
            shape,
            radius,
            left: spread,
            right: spread,
            top: (spread - offset.floor() as i32).max(0),
            bottom: spread + offset.ceil().max(0.0) as i32,
            shadow_offset: offset,
            shadow_blur: blur,
        }
    }

    /// The backdrop window's size.
    pub fn window(&self) -> (i32, i32) {
        (
            self.shape
                .0
                .saturating_add(self.left)
                .saturating_add(self.right),
            self.shape
                .1
                .saturating_add(self.top)
                .saturating_add(self.bottom),
        )
    }

    /// The rounding of the window region that takes the clicks: a little tighter than the
    /// drawn corner, so the region (which has no anti-aliasing) never trims the shape's soft edge.
    pub fn region_radius(&self) -> i32 {
        (self.radius - 2.0).max(0.0).round() as i32
    }
}

/// The egui frame for a surface's content. Over composed glass it paints nothing (the backdrop
/// window draws the surface); without it, egui paints the surface itself: the tint made nearly
/// solid (nothing blurs behind it), the rounded shape and the rim. There is no shadow then, since
/// the window is only as big as the shape.
pub fn content_frame(composed: bool, style: &Style) -> egui::Frame {
    if composed {
        return egui::Frame::new();
    }
    egui::Frame::new()
        .fill(theme::mix(style.solid, style.tint, 0.5))
        .corner_radius(style.radius.round().clamp(0.0, 255.0) as u8)
        .stroke(egui::Stroke::new(
            1.0_f32,
            with_alpha(style.rim, style.rim.a() / 2),
        ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn style(radius: f32) -> Style {
        Style {
            radius,
            luminosity: Luminosity::NONE,
            tint: Color32::BLACK,
            rim: Color32::WHITE,
            shadow: Shadow {
                offset_y: 8.0,
                blur: 32.0,
                color: Color32::BLACK,
            },
            solid: Color32::BLACK,
        }
    }

    #[test]
    fn points_become_pixels_at_each_scale() {
        let g = Geometry::new((1080, 840), 1.5, &style(24.0));
        assert_eq!(g.radius, 36.0);
        assert_eq!((g.left, g.right), (96, 96));
        // The shadow falls 12 px down: less room above, more below.
        assert_eq!((g.top, g.bottom), (84, 108));
        assert_eq!(g.window(), (1080 + 192, 840 + 192));
        let g = Geometry::new((2160, 1680), 3.0, &style(24.0));
        assert_eq!(g.radius, 72.0);
        assert_eq!((g.left, g.top, g.bottom), (192, 168, 216));
    }

    #[test]
    fn radius_fits_the_shape() {
        let g = Geometry::new((100, 40), 2.0, &style(28.0));
        assert_eq!(g.radius, 20.0);
        let g = Geometry::new((0, 0), 1.0, &style(28.0));
        assert_eq!(g.radius, 0.0);
        assert_eq!(g.region_radius(), 0);
    }

    #[test]
    fn click_region_is_never_rounder_than_the_drawn_shape() {
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let g = Geometry::new((800, 600), scale, &style(24.0));
            assert!(g.region_radius() as f32 <= g.radius);
            assert!(g.region_radius() as f32 >= g.radius - 2.5);
        }
    }

    #[test]
    fn theme_style_follows_mode_and_glass() {
        let accent = Color32::from_rgb(0, 103, 192);
        let dark = Style::new(&Colors::new(false, accent), 0.5, 24.0);
        let light = Style::new(&Colors::new(true, accent), 0.5, 24.0);
        assert!(
            dark.tint.to_srgba_unmultiplied()[0] < 80
                && light.tint.to_srgba_unmultiplied()[0] > 200
        );
        // As much hidden as a glass window's own tint.
        assert_eq!(dark.tint.a(), theme::shell(false, accent, 0.5).a());
        assert_eq!(light.tint.a(), theme::shell(true, accent, 0.5).a());
        // A dark tint needs nothing added to the acrylic under it.
        let acrylic = Luminosity::acrylic(false);
        assert!(close(dark.luminosity.lift, acrylic.lift) && dark.luminosity.keep == acrylic.keep);
        // Light mode's shadow is lighter.
        assert!(light.shadow.color.a() < dark.shadow.color.a());
        assert_eq!(dark.radius, 24.0);
        // More glass, a thinner tint.
        let clear = Style::new(&Colors::new(false, accent), 1.0, 24.0);
        let solid = Style::new(&Colors::new(false, accent), 0.0, 24.0);
        assert!(clear.tint.a() < dark.tint.a() && dark.tint.a() < solid.tint.a());
    }

    fn close(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-4)
    }

    /// The matrix as Direct2D applies it (without the clamp).
    fn through(m: &[f32; 20], rgb: [f32; 3]) -> [f32; 3] {
        let input = [rgb[0], rgb[1], rgb[2], 1.0, 1.0];
        std::array::from_fn(|j| (0..5).map(|i| input[i] * m[i * 4 + j]).sum())
    }

    #[test]
    fn luminosity_squeezes_brightness_and_keeps_colour() {
        let none = Luminosity::NONE;
        assert!(close(none.apply([0.2, 0.5, 0.9]), [0.2, 0.5, 0.9]));
        let dark = Luminosity::acrylic(false);
        // Black is lifted, white held down, grey in between on a straight line.
        let grey = |v: f32| dark.apply([v; 3])[0];
        assert!((grey(0.0) - dark.lift[0]).abs() < 1e-4);
        assert!((grey(1.0) - (dark.lift[0] + dark.keep)).abs() < 1e-4);
        assert!((grey(0.5) - (grey(0.0) + grey(1.0)) / 2.0).abs() < 1e-4);
        // A blue keeps its colour: each channel's distance from the brightness is the same.
        let blue = [60.0 / 255.0, 154.0 / 255.0, 232.0 / 255.0];
        let out = dark.apply(blue);
        let y = |c: [f32; 3]| c.iter().zip(LUMA).map(|(c, w)| c * w).sum::<f32>();
        for i in 0..3 {
            assert!(((out[i] - y(out)) - (blue[i] - y(blue))).abs() < 1e-4);
        }
        // Light mode lifts black further.
        assert!(Luminosity::acrylic(true).lift[0] > dark.lift[0]);
    }

    /// What a style shows over a solid backdrop, 0 to 255.
    fn seen(s: &Style, back: [f32; 3]) -> [f32; 3] {
        let [r, g, b, a] = s.tint.to_srgba_unmultiplied().map(|v| v as f32 / 255.0);
        let under = s.luminosity.apply(back);
        std::array::from_fn(|i| ([r, g, b][i] * a + under[i] * (1.0 - a)) * 255.0)
    }

    #[test]
    fn matches_what_files_shows() {
        // Files' sidebar, measured with a black accent, over solid windows (dark and light).
        let cases = [
            (false, 0.0, [0.0; 3], 14.0),
            (false, 0.325, [0.0; 3], 21.0),
            (false, 1.0, [0.0; 3], 35.0),
            (false, 0.325, [1.0; 3], 45.0),
            (false, 1.0, [1.0; 3], 87.0),
            (false, 1.0, [0.5; 3], 61.0),
            (true, 0.0, [0.0; 3], 228.0),
            (true, 1.0, [0.0; 3], 231.0),
            (true, 0.0, [1.0; 3], 239.0),
            (true, 1.0, [0.25; 3], 244.0),
            (true, 1.0, [1.0; 3], 255.0),
        ];
        for (light, glass, back, files) in cases {
            let s = Style::new(&Colors::new(light, Color32::BLACK), glass, 24.0);
            let v = seen(&s, back)[0];
            assert!(
                (v - files).abs() <= 3.0,
                "light {light} glass {glass} {back:?}: {v} vs {files}"
            );
        }
    }

    #[test]
    fn a_dark_tint_is_shown_as_it_is() {
        let (tint, extra) = as_window_shows(Color32::from_rgba_unmultiplied(10, 10, 10, 188));
        assert!(tint.to_srgba_unmultiplied()[..3]
            .iter()
            .all(|&v| v.abs_diff(10) <= 1));
        assert_eq!(tint.a(), 188);
        assert!(extra.iter().all(|&e| e < 0.001));
        // A light one lighter: past white, so the rest lifts the backdrop.
        let (tint, extra) = as_window_shows(Color32::from_rgba_unmultiplied(224, 224, 224, 110));
        assert!(tint.to_srgba_unmultiplied()[0] >= 250);
        assert!(extra.iter().all(|&e| e > 0.1));
    }

    #[test]
    fn matrix_matches_apply() {
        for l in [
            Luminosity::NONE,
            Luminosity::acrylic(false),
            Luminosity::acrylic(true),
        ] {
            let m = l.matrix();
            assert_eq!(m[15], 1.0, "alpha passes through");
            assert_eq!(m[19], 0.0);
            for rgb in [[0.0; 3], [1.0; 3], [0.2, 0.5, 0.9], [0.5, 0.1, 0.1]] {
                let raw = through(&m, rgb).map(|c| c.clamp(0.0, 1.0));
                assert!(close(raw, l.apply(rgb)), "{l:?} {rgb:?}");
            }
        }
    }

    /// Contrast ratio between two opaque sRGB colours (WCAG).
    fn contrast(a: [f32; 3], b: [f32; 3]) -> f32 {
        let lin = |c: f32| {
            if c <= 0.04045 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        let l = |c: [f32; 3]| c.iter().zip(LUMA).map(|(c, w)| lin(*c) * w).sum::<f32>();
        let (x, y) = (l(a), l(b));
        (x.max(y) + 0.05) / (x.min(y) + 0.05)
    }

    #[test]
    fn text_stays_legible_over_black_and_white_at_any_glass() {
        for light in [false, true] {
            for accent in [Color32::BLACK, Color32::from_rgb(0, 103, 192)] {
                let c = Colors::new(light, accent);
                let text = c.text.to_normalized_gamma_f32();
                for glass in [0.0, 0.165, 0.325, 0.5, 0.75, 1.0] {
                    let s = Style::new(&c, glass, 24.0);
                    let [r, g, b, a] = s.tint.to_srgba_unmultiplied().map(|v| v as f32 / 255.0);
                    for back in [[0.0; 3], [1.0; 3]] {
                        let under = s.luminosity.apply(back);
                        let seen: [f32; 3] =
                            std::array::from_fn(|i| [r, g, b][i] * a + under[i] * (1.0 - a));
                        let ratio = contrast([text[0], text[1], text[2]], seen);
                        assert!(
                            ratio >= 4.5,
                            "light {light} glass {glass} over {back:?}: {ratio}"
                        );
                    }
                }
            }
        }
    }
}
