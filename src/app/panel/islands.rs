//! Islands: several separate pieces of glass under one window, with the desktop showing between
//! them. The window itself is transparent and stays whole (the gaps are still the window's, so
//! they can move it); each island is a rounded piece of the panel's glass.
//!
//! An app lays its islands out in egui points every frame, hands them to
//! [`super::GlassPanel::set_islands`], and draws its content on top. The panel changes only what
//! moved. Where composition is not available, [`paint_solid`] paints the islands in egui
//! instead, as solid rounded fills.

use super::style::{content_frame, Style};
use crate::app::glass_theme as theme;

/// One piece of glass, in points of the window's client area.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Island {
    pub rect: egui::Rect,
    /// Corner radius in points; never more than half the shorter side.
    pub radius: f32,
    /// How much of the glass shows, 0 to 1. Less than 1 lets the sharp desktop through, for a
    /// faint backing behind content that has its own surfaces.
    pub opacity: f32,
    /// Whether it casts the style's shadow.
    pub shadow: bool,
}

impl Island {
    /// A full piece of glass with a shadow.
    pub fn new(rect: egui::Rect, radius: f32) -> Self {
        Self {
            rect,
            radius,
            opacity: 1.0,
            shadow: true,
        }
    }

    /// A pill: as round as the rectangle allows.
    pub fn pill(rect: egui::Rect) -> Self {
        Self::new(rect, rect.height().min(rect.width()) / 2.0)
    }

    /// The same island, only `opacity` of it showing and without a shadow.
    pub fn faint(self, opacity: f32) -> Self {
        Self {
            opacity,
            shadow: false,
            ..self
        }
    }
}

/// An island's rectangle (left, top, right, bottom) and radius, in physical pixels of the client
/// area at `scale` pixels per point. Edges are rounded to whole pixels the way egui places its
/// own drawing, so the glass and the content painted on it line up.
pub fn to_pixels(island: &Island, scale: f32) -> ([i32; 4], f32) {
    use super::guard::{finite, MAX_COORD};
    let r = island.rect;
    let scale = super::guard::scale(scale);
    // NaN becomes 0 and anything huge stays far from overflowing i32 in the sums made later.
    let limit = (MAX_COORD * 4) as f32;
    let px = |v: f32| finite(v * scale, -limit, limit, 0.0).round() as i32;
    let edges = [px(r.left()), px(r.top()), px(r.right()), px(r.bottom())];
    let (w, h) = (edges[2] - edges[0], edges[3] - edges[1]);
    let radius = finite(
        island.radius * scale,
        0.0,
        (w.min(h) as f32 / 2.0).max(0.0),
        0.0,
    );
    (edges, radius)
}

/// The colour to paint so that, on a transparent window over the desktop, it shows as `css` would
/// laid over it in a browser (straight alpha, in sRGB). egui hands the window its colours
/// premultiplied in linear light, which Windows then lays over in sRGB, so a faint white painted
/// as it is shows far lighter (white at 4% adds what 22% would). Dark colours barely change.
pub fn shows_as(css: egui::Color32) -> egui::Color32 {
    use egui::ecolor::{gamma_from_linear, linear_from_gamma};
    let [r, g, b, a] = css.to_srgba_unmultiplied();
    let alpha = a as f32 / 255.0;
    if alpha <= 0.0 || alpha >= 1.0 {
        return css;
    }
    let channel = |v: u8| {
        let added = v as f32 / 255.0 * alpha;
        let straight = gamma_from_linear(linear_from_gamma(added) / alpha).min(1.0);
        (straight * 255.0).round() as u8
    };
    egui::Color32::from_rgba_unmultiplied(channel(r), channel(g), channel(b), a)
}

/// The islands as egui shapes, for a window without composed glass: each a nearly solid rounded
/// fill with a faint rim (see [`content_frame`]). Paint them under the content, e.g. into a
/// `Shape::Noop` reserved at the start of the frame (`painter.set(idx, Shape::Vec(..))`).
pub fn solid_shapes(islands: &[Island], style: &Style) -> Vec<egui::Shape> {
    let frame = content_frame(false, style);
    islands
        .iter()
        .map(|island| {
            let radius = island
                .radius
                .min(island.rect.height().min(island.rect.width()) / 2.0);
            let fill = theme::with_alpha(
                frame.fill,
                (frame.fill.a() as f32 * island.opacity.clamp(0.0, 1.0)).round() as u8,
            );
            egui::Shape::Rect(egui::epaint::RectShape::new(
                island.rect,
                radius.round().clamp(0.0, 255.0) as u8,
                fill,
                frame.stroke,
                egui::StrokeKind::Inside,
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{pos2, Rect};

    #[test]
    fn points_become_whole_pixels() {
        let i = Island::new(
            Rect::from_min_max(pos2(10.0, 10.3), pos2(110.0, 60.0)),
            12.0,
        );
        let (edges, radius) = to_pixels(&i, 1.5);
        assert_eq!(edges, [15, 15, 165, 90]);
        assert_eq!(radius, 18.0);
    }

    #[test]
    fn a_pill_is_half_its_height_round() {
        let i = Island::pill(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(300.0, 52.0)));
        assert_eq!(i.radius, 26.0);
        // A tall narrow pill (the rail) takes half its width.
        let rail = Island::pill(Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(60.0, 400.0)));
        assert_eq!(rail.radius, 30.0);
        let (_, r) = to_pixels(&rail, 2.0);
        assert_eq!(r, 60.0);
    }

    #[test]
    fn radius_never_passes_half_the_shorter_side() {
        let i = Island::new(
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(40.0, 20.0)),
            99.0,
        );
        assert_eq!(to_pixels(&i, 1.25).1, 12.5);
        let empty = Island::new(
            Rect::from_min_size(pos2(5.0, 5.0), egui::vec2(0.0, 0.0)),
            8.0,
        );
        assert_eq!(to_pixels(&empty, 2.0).1, 0.0);
    }

    #[test]
    fn hostile_islands_give_finite_whole_pixels() {
        let bad = [
            Island::new(
                Rect::from_min_max(pos2(f32::NAN, 0.0), pos2(10.0, f32::INFINITY)),
                f32::NAN,
            ),
            Island::new(
                Rect::from_min_max(pos2(-1e30, -1e30), pos2(1e30, 1e30)),
                1e30,
            ),
            Island::new(Rect::from_min_max(pos2(50.0, 50.0), pos2(10.0, 10.0)), 8.0),
        ];
        for i in &bad {
            for scale in [0.0, -1.0, f32::NAN, f32::INFINITY, 1.0, 2.5] {
                let (e, r) = to_pixels(i, scale);
                assert!(r.is_finite() && r >= 0.0, "{i:?} at {scale}: {r}");
                // Widths can be taken without overflow.
                let _ = (e[2] - e[0], e[3] - e[1]);
            }
        }
    }

    /// What a colour painted by egui adds over black on a transparent window, 0 to 1.
    fn added(c: egui::Color32) -> f32 {
        use egui::ecolor::{gamma_from_linear, linear_from_gamma};
        let [r, _, _, a] = c.to_srgba_unmultiplied();
        gamma_from_linear(linear_from_gamma(r as f32 / 255.0) * a as f32 / 255.0)
    }

    #[test]
    fn a_faint_white_shows_as_faint_as_in_css() {
        let css = egui::Color32::from_white_alpha(10);
        let paint = shows_as(css);
        assert_eq!(paint.a(), 10);
        // Painted as it is, it adds several times what CSS would; converted, the same.
        assert!(added(css) > 4.0 * 10.0 / 255.0);
        assert!((added(paint) - 10.0 / 255.0).abs() < 0.004);
        // Opaque and clear colours pass through.
        assert_eq!(shows_as(egui::Color32::RED), egui::Color32::RED);
        assert_eq!(
            shows_as(egui::Color32::TRANSPARENT),
            egui::Color32::TRANSPARENT
        );
    }

    #[test]
    fn faint_drops_the_shadow() {
        let i = Island::new(
            Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(10.0, 10.0)),
            4.0,
        );
        let f = i.faint(0.4);
        assert!(!f.shadow && f.opacity == 0.4 && f.rect == i.rect);
        assert!(i.shadow && i.opacity == 1.0);
    }
}
