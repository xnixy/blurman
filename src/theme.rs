//! Colors, spacing, and the small widgets the window is built from.
//!
//! The palette matches the pump screen app: a plum shell, rose accent, and rounded cards over the
//! window's frosted backdrop. It comes in dark and light, following the Windows app mode.

use egui::{Color32, CornerRadius, Frame, Margin, RichText, Shadow, Stroke, Theme, Ui, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};

/// Rose. Strong enough to carry white text on dark and light alike.
pub const ACCENT: Color32 = Color32::from_rgb(196, 54, 78);

/// Every color that differs between dark and light.
#[derive(Clone, Copy)]
pub struct Colors {
    pub light: bool,
    /// Behind everything when Windows cannot frost the window.
    pub bg: Color32,
    /// Tooltips, menus, and dialogs, which float over content and need to stay readable.
    pub popup: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub warn: Color32,
    pub ok: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    /// Thin outlines on cards, dialogs, and switches.
    pub hairline: Color32,
    /// The well behind a row of joined buttons.
    pub well: Color32,
    /// A switch that is off.
    pub track: Color32,
    /// Text on a button for something that cannot be undone.
    pub danger_text: Color32,
    card: Color32,
    shadow: Color32,
    sidebar_glass: Color32,
    sidebar_solid: Color32,
    page_glass: Color32,
}

impl Colors {
    fn dark() -> Self {
        Self {
            light: false,
            bg: Color32::from_rgb(16, 14, 20),
            popup: Color32::from_rgb(34, 28, 36),
            text: Color32::from_rgb(240, 236, 238),
            muted: Color32::from_rgb(186, 180, 186),
            warn: Color32::from_rgb(230, 186, 120),
            ok: Color32::from_rgb(126, 196, 154),
            row_hover: Color32::from_rgba_premultiplied(18, 18, 18, 18),
            // Rose at about 22% opacity.
            row_selected: Color32::from_rgba_premultiplied(43, 12, 17, 56),
            hairline: Color32::from_rgba_unmultiplied(255, 255, 255, 32),
            well: Color32::from_rgba_unmultiplied(255, 255, 255, 12),
            track: Color32::from_rgb(70, 60, 70),
            danger_text: Color32::from_rgb(255, 196, 204),
            card: Color32::from_rgba_unmultiplied(34, 28, 36, 214),
            shadow: Color32::from_black_alpha(80),
            sidebar_glass: Color32::from_rgba_unmultiplied(10, 8, 12, 178),
            sidebar_solid: Color32::from_rgb(12, 10, 15),
            page_glass: Color32::from_rgba_unmultiplied(16, 12, 18, 140),
        }
    }

    /// The same plum family, lifted to near white, with text dark enough to read over light acrylic.
    fn light() -> Self {
        Self {
            light: true,
            bg: Color32::from_rgb(242, 238, 241),
            popup: Color32::from_rgb(252, 250, 251),
            text: Color32::from_rgb(30, 24, 30),
            muted: Color32::from_rgb(96, 88, 96),
            warn: Color32::from_rgb(160, 96, 0),
            ok: Color32::from_rgb(24, 124, 74),
            row_hover: Color32::from_black_alpha(14),
            // Rose at about 16% opacity.
            row_selected: Color32::from_rgba_premultiplied(31, 9, 12, 40),
            hairline: Color32::from_black_alpha(28),
            well: Color32::from_black_alpha(12),
            track: Color32::from_rgb(190, 182, 190),
            danger_text: Color32::from_rgb(150, 24, 48),
            card: Color32::from_rgba_unmultiplied(255, 252, 254, 200),
            shadow: Color32::from_black_alpha(26),
            sidebar_glass: Color32::from_rgba_unmultiplied(232, 226, 232, 170),
            sidebar_solid: Color32::from_rgb(232, 227, 231),
            page_glass: Color32::from_rgba_unmultiplied(248, 244, 247, 120),
        }
    }
}

static LIGHT: AtomicBool = AtomicBool::new(false);

/// The colors for the mode the window is in now.
pub fn colors() -> Colors {
    if LIGHT.load(Ordering::Relaxed) {
        Colors::light()
    } else {
        Colors::dark()
    }
}

/// Switch the palette. True when it changed.
pub fn set_light(light: bool) -> bool {
    LIGHT.swap(light, Ordering::Relaxed) != light
}

/// Give egui both looks. It picks one by the Windows app mode and switches as that changes.
pub fn apply(ctx: &egui::Context) {
    for (theme, colors) in [
        (Theme::Dark, Colors::dark()),
        (Theme::Light, Colors::light()),
    ] {
        let mut style = (*ctx.style_of(theme)).clone();
        style.visuals = shell_visuals(&colors);
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        // Selectable text would take clicks meant for the app rows it sits on.
        style.interaction.selectable_labels = false;
        ctx.set_style_of(theme, style);
    }
}

fn shell_visuals(c: &Colors) -> egui::Visuals {
    // Each pair is (dark, light).
    let pick = |dark: Color32, light: Color32| if c.light { light } else { dark };
    let mut visuals = if c.light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.window_fill = c.popup;
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.faint_bg_color = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 14),
        Color32::from_black_alpha(10),
    );
    visuals.extreme_bg_color = pick(
        Color32::from_rgba_unmultiplied(18, 14, 20, 170),
        Color32::from_rgba_unmultiplied(255, 255, 255, 210),
    );
    visuals.widgets.noninteractive.fg_stroke.color = pick(
        Color32::from_rgb(176, 170, 176),
        Color32::from_rgb(70, 64, 70),
    );
    visuals.widgets.noninteractive.bg_stroke.color = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 28),
        Color32::from_black_alpha(26),
    );
    let mut idle = visuals.widgets.inactive;
    idle.bg_fill = pick(
        Color32::from_rgb(58, 50, 58),
        Color32::from_rgb(222, 215, 220),
    );
    idle.weak_bg_fill = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 18),
        Color32::from_black_alpha(14),
    );
    idle.bg_stroke = Stroke::new(
        1.0_f32,
        pick(
            Color32::from_rgba_unmultiplied(255, 255, 255, 36),
            Color32::from_black_alpha(32),
        ),
    );
    idle.corner_radius = CornerRadius::same(8);
    idle.fg_stroke.color = c.text;
    let mut hovered = idle;
    hovered.weak_bg_fill = pick(
        Color32::from_rgba_unmultiplied(255, 255, 255, 30),
        Color32::from_black_alpha(26),
    );
    hovered.bg_fill = pick(
        Color32::from_rgb(78, 66, 74),
        Color32::from_rgb(208, 199, 206),
    );
    let mut active = hovered;
    active.bg_fill = ACCENT;
    active.weak_bg_fill = ACCENT;
    active.fg_stroke.color = Color32::WHITE;
    visuals.widgets.inactive = idle;
    visuals.widgets.hovered = hovered;
    visuals.widgets.active = active;
    visuals.widgets.open = hovered;
    visuals.selection.bg_fill = pick(
        Color32::from_rgb(190, 52, 74),
        Color32::from_rgb(232, 150, 164),
    );
    visuals.selection.stroke = Stroke::new(
        1.0_f32,
        pick(
            Color32::from_rgb(230, 140, 152),
            Color32::from_rgb(176, 40, 62),
        ),
    );
    visuals.hyperlink_color = pick(
        Color32::from_rgb(255, 176, 186),
        Color32::from_rgb(176, 32, 58),
    );
    visuals.window_corner_radius = CornerRadius::same(12);
    visuals.menu_corner_radius = CornerRadius::same(10);
    visuals.window_stroke = Stroke::new(1.0_f32, c.hairline);
    visuals
}

/// The sidebar: set apart from the pages, darker in dark mode and a shade deeper in light.
pub fn sidebar(glass: bool) -> Frame {
    let c = colors();
    Frame::new()
        .fill(if glass {
            c.sidebar_glass
        } else {
            c.sidebar_solid
        })
        .inner_margin(Margin::symmetric(14, 16))
}

/// The tint laid over the acrylic backdrop behind each page. Opaque when Windows has no backdrop.
pub fn page(glass: bool) -> Frame {
    let c = colors();
    Frame::new()
        .fill(if glass { c.page_glass } else { c.bg })
        .inner_margin(Margin::symmetric(22, 18))
}

/// A question that needs an answer before anything else.
pub fn dialog() -> Frame {
    let c = colors();
    Frame::new()
        .fill(c.popup)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(14))
        .inner_margin(Margin::same(20))
}

pub fn card<R>(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    let c = colors();
    Frame::new()
        .fill(c.card)
        .stroke(Stroke::new(1.0_f32, c.hairline))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(Margin::same(14))
        .outer_margin(Margin::symmetric(4, 0))
        .shadow(Shadow {
            offset: [0, 8],
            blur: 18,
            spread: 0,
            color: c.shadow,
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add_contents(ui)
        })
        .inner
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(colors().muted)
}

pub fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).strong().color(Color32::WHITE))
        .fill(ACCENT)
        .corner_radius(CornerRadius::same(10))
        .min_size(Vec2::new(140.0, 34.0))
}

/// A choice in a row, filled with the accent while it is the current one.
pub fn choice(ui: &mut Ui, selected: bool, label: &str) -> egui::Response {
    let text = if selected {
        RichText::new(label).color(Color32::WHITE)
    } else {
        RichText::new(label)
    };
    let mut button = egui::Button::new(text);
    if selected {
        button = button.fill(ACCENT);
    }
    ui.add(button)
}
