//! Colors, fonts, and how the window is drawn: the look Blurman shares with Espalier and
//! shellman.
//!
//! Neutrals carry a faint tint of the accent, and the accent only fills or colors something
//! where it stands out from the page (`app::look`). Cards, panes, the sidebar, and the header are
//! islands: on real glass where Windows composes it, on one frosted window, or on a solid page.

use crate::app::glass_theme;
use crate::app::look::Look;
pub use crate::app::look::{mix, with_alpha, ISLAND_RADIUS, PILL_H};
use egui::{Color32, CornerRadius, FontFamily, FontId, RichText, Stroke, Vec2};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

/// How the window is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Only the islands show, each on real glass. The window between them is invisible.
    Glass,
    /// One acrylic window behind all the islands.
    Frosted,
    /// Nothing see-through.
    Solid,
    /// Windows' high contrast theme: solid, in the system's colors.
    HighContrast,
}

impl Mode {
    /// The mode for now, from the window setting ("glass", "frosted", or "solid"). High
    /// contrast, Remote Desktop, and Windows' own transparency switch overrule it.
    pub fn now(style: &str) -> Mode {
        if high_contrast() {
            Mode::HighContrast
        } else if forced_solid() || style == "solid" {
            Mode::Solid
        } else if style == "frosted" {
            Mode::Frosted
        } else {
            Mode::Glass
        }
    }
}

/// Windows composes neither glass nor acrylic here, or was told not to.
pub fn forced_solid() -> bool {
    remote_session() || transparency_off() || std::env::var_os("BLURMAN_SOLID").is_some()
}

/// Windows' "Transparency effects" (Settings, Personalization, Colors) are off.
pub fn transparency_off() -> bool {
    read_dword(
        windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
        windows::core::w!("EnableTransparency"),
    ) == Some(0)
}

fn remote_session() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};
    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

/// Windows' high contrast theme is on. BLURMAN_HIGH_CONTRAST tries it without changing Windows.
pub fn high_contrast() -> bool {
    use windows::Win32::UI::Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW};
    use windows::Win32::UI::WindowsAndMessaging::{
        SystemParametersInfoW, SPI_GETHIGHCONTRAST, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    };
    if std::env::var_os("BLURMAN_HIGH_CONTRAST").is_some() {
        return true;
    }
    let mut state = HIGHCONTRASTW {
        cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
        ..Default::default()
    };
    let read = unsafe {
        SystemParametersInfoW(
            SPI_GETHIGHCONTRAST,
            state.cbSize,
            Some(&mut state as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
    };
    read.is_ok() && state.dwFlags.contains(HCF_HIGHCONTRASTON)
}

/// Windows' "Text size" (Settings, Accessibility), as a factor from 1.0 to 2.25.
/// BLURMAN_TEXT_SCALE (100 to 225) tries a size without changing Windows.
pub fn text_scale() -> f32 {
    let percent = std::env::var("BLURMAN_TEXT_SCALE")
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .or_else(|| {
            read_dword(
                windows::core::w!(r"Software\Microsoft\Accessibility"),
                windows::core::w!("TextScaleFactor"),
            )
        })
        .unwrap_or(100);
    percent.clamp(100, 225) as f32 / 100.0
}

fn read_dword(key: windows::core::PCWSTR, value: windows::core::PCWSTR) -> Option<u32> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
    let mut data = 0u32;
    let mut size = 4u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key,
            value,
            RRF_RT_REG_DWORD,
            None,
            Some(&mut data as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    (err == ERROR_SUCCESS).then_some(data)
}

/// Accent colors to choose instead of Windows' own: a name, the shade for dark mode, and the
/// shade for light mode. Light on dark and deep on light, so text on them stays readable.
pub const ACCENTS: [(&str, [u8; 3], [u8; 3]); 8] = [
    ("Blue", [0x60, 0xcd, 0xff], [0x00, 0x5f, 0xb8]),
    ("Purple", [0xc5, 0xa8, 0xff], [0x6b, 0x3f, 0xc0]),
    ("Pink", [0xff, 0x9f, 0xd4], [0xb4, 0x00, 0x6e]),
    ("Red", [0xff, 0x99, 0xa4], [0xc4, 0x2b, 0x1c]),
    ("Orange", [0xff, 0xb4, 0x6b], [0xb5, 0x4a, 0x00]),
    ("Yellow", [0xff, 0xd8, 0x66], [0x8a, 0x6a, 0x00]),
    ("Green", [0x6c, 0xcb, 0x5f], [0x0f, 0x7b, 0x0f]),
    ("Graphite", [0xc8, 0xc8, 0xc8], [0x5a, 0x5a, 0x5a]),
];

/// The accent for a setting: one of `ACCENTS` by name, or anything else for Windows' own.
pub fn accent_for(setting: &str, light: bool) -> Color32 {
    let named = ACCENTS
        .iter()
        .find(|(name, ..)| name.eq_ignore_ascii_case(setting));
    match named {
        Some((_, on_dark, on_light)) => {
            let [r, g, b] = if light { *on_light } else { *on_dark };
            Color32::from_rgb(r, g, b)
        }
        None => windows_accent(light),
    }
}

/// The Windows accent, in the shade Windows uses for controls in this mode.
fn windows_accent(light: bool) -> Color32 {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};
    // Eight RGBA shades, light to dark.
    let mut palette = [0u8; 32];
    let mut size = palette.len() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            windows::core::w!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\Accent"),
            windows::core::w!("AccentPalette"),
            RRF_RT_REG_BINARY,
            None,
            Some(palette.as_mut_ptr().cast()),
            Some(&mut size),
        )
    };
    if err != ERROR_SUCCESS || size < 32 {
        return accent_for("Blue", light);
    }
    let at = if light { 4 } else { 1 } * 4;
    Color32::from_rgb(palette[at], palette[at + 1], palette[at + 2])
}

/// Every color the window draws with, for the mode and accent it is in now.
#[derive(Clone, Copy, Debug)]
pub struct Colors {
    pub light: bool,
    pub high_contrast: bool,
    /// The accent where it fills something, and text on that fill.
    pub accent: Color32,
    pub on_accent: Color32,
    /// The accent adjusted to read as text on the page.
    pub accent_text: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub hairline: Color32,
    /// A switch that is off.
    pub track: Color32,
    /// Islands where there is no glass under them.
    pub island: Color32,
    /// Pills: buttons, chips, and text fields, and their hovered fill.
    pub control: Color32,
    pub control_hover: Color32,
    /// The soft fill of a secondary button.
    pub soft: Color32,
    pub row_hover: Color32,
    pub row_selected: Color32,
    pub danger: Color32,
    pub danger_text: Color32,
    pub warn: Color32,
    pub ok: Color32,
    /// The page behind the islands when nothing is see-through.
    pub bg: Color32,
    /// Tooltips, menus, and dialogs, which egui draws over no glass.
    pub popup: Color32,
}

impl Colors {
    fn new(light: bool, accent: Color32, high_contrast: bool) -> Self {
        let look = if high_contrast {
            Look::high_contrast()
        } else {
            Look::new(light, accent)
        };
        let light = look.light;
        let grey = |grey: u8, amount: f32| mix(Color32::from_gray(grey), accent, amount);
        let bg = if high_contrast {
            look.island
        } else if light {
            grey(242, 0.05)
        } else {
            grey(16, 0.06)
        };
        let (soft, warn, ok, danger_text) = if high_contrast {
            (look.island, look.text, look.text, look.text)
        } else if light {
            (
                Color32::from_white_alpha(150),
                Color32::from_rgb(150, 90, 0),
                Color32::from_rgb(15, 123, 15),
                Color32::from_rgb(215, 0, 21),
            )
        } else {
            (
                // A deep tinted fill, not a wash of white: over a see-through window white
                // lifts toward grey.
                with_alpha(grey(54, 0.09), 215),
                Color32::from_rgb(240, 190, 110),
                Color32::from_rgb(108, 203, 95),
                Color32::from_rgb(255, 69, 58),
            )
        };
        Self {
            light,
            high_contrast,
            accent: look.accent_fill,
            on_accent: look.on_accent,
            accent_text: look.accent_text,
            text: look.text,
            muted: look.muted,
            hairline: look.hairline,
            track: look.track,
            island: look.island,
            control: look.control,
            control_hover: look.control_hover,
            soft,
            row_hover: look.row_hover,
            row_selected: look.row_selected,
            danger: look.danger,
            danger_text,
            warn,
            ok,
            bg,
            popup: Color32::from_rgb(look.island.r(), look.island.g(), look.island.b()),
        }
    }
}

struct Current {
    colors: Colors,
    mode: Mode,
    /// The islands are on real glass, so they paint no fill of their own.
    composed: bool,
}

static CURRENT: RwLock<Option<Current>> = RwLock::new(None);
static HAS_ICONS: AtomicBool = AtomicBool::new(false);

/// Set the mode, accent, and window style everything is drawn with from now on.
pub fn set(light: bool, accent_setting: &str, mode: Mode, composed: bool) -> Colors {
    let high_contrast = mode == Mode::HighContrast;
    let accent = accent_for(accent_setting, light);
    let colors = Colors::new(light, accent, high_contrast);
    glass_theme::set(colors.light, accent);
    if let Ok(mut current) = CURRENT.write() {
        *current = Some(Current {
            colors,
            mode,
            composed,
        });
    }
    colors
}

fn current<R>(read: impl FnOnce(&Current) -> R) -> R {
    if let Ok(current) = CURRENT.read() {
        if let Some(current) = current.as_ref() {
            return read(current);
        }
    }
    read(&Current {
        colors: Colors::new(false, accent_for("Blue", false), false),
        mode: Mode::Solid,
        composed: false,
    })
}

pub fn colors() -> Colors {
    current(|current| current.colors)
}

/// What an island paints under its content, and the line around it.
///
/// On real glass in dark mode, nothing: light text keeps its weight over clear pixels. On light
/// glass, a nearly opaque white: grey text anti-aliased onto clear pixels turns pale otherwise.
/// With no glass, a tinted fill and a hairline.
pub fn island() -> (Color32, Stroke) {
    current(|current| {
        let c = &current.colors;
        if c.high_contrast {
            (c.island, Stroke::new(1.0_f32, c.text))
        } else if current.composed {
            let fill = if c.light {
                Color32::from_white_alpha(225)
            } else {
                Color32::TRANSPARENT
            };
            (fill, Stroke::NONE)
        } else {
            (c.island, Stroke::new(1.0_f32, c.hairline))
        }
    })
}

/// Whether islands are real glass, to be registered with the glass panel.
pub fn composed() -> bool {
    current(|current| current.composed)
}

/// What fills the window between the islands.
pub fn page_fill() -> Color32 {
    current(|current| {
        let c = &current.colors;
        match current.mode {
            Mode::HighContrast | Mode::Solid => c.bg,
            Mode::Glass if current.composed => Color32::TRANSPARENT,
            // One even tint over the acrylic, letting it through the same everywhere.
            _ if c.light => with_alpha(mix(Color32::from_gray(248), c.accent, 0.03), 120),
            _ => with_alpha(mix(Color32::from_gray(16), c.accent, 0.06), 140),
        }
    })
}

/// Segoe UI for text, Segoe UI Semibold for headings, and Segoe Fluent Icons for glyphs.
pub fn install_fonts(ctx: &egui::Context) {
    use egui::{FontData, FontDefinitions};
    let mut fonts = FontDefinitions::default();
    let dir = std::env::var("WINDIR").unwrap_or_else(|_| r"C:\Windows".into());
    let read = |file: &str| std::fs::read(format!(r"{dir}\Fonts\{file}")).ok();
    let add = |fonts: &mut FontDefinitions, name: &str, bytes: Vec<u8>| {
        fonts
            .font_data
            .insert(name.into(), Arc::new(FontData::from_owned(bytes)));
    };
    if let Some(bytes) = ["SegUIVar.ttf", "segoeui.ttf"].into_iter().find_map(read) {
        add(&mut fonts, "system", bytes);
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system".into());
    }
    if let Some(bytes) = read("consola.ttf") {
        add(&mut fonts, "mono", bytes);
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "mono".into());
    }
    // App names and window titles can be in any script.
    for (name, file) in [
        ("segoe", "segoeui.ttf"),
        ("symbol", "seguisym.ttf"),
        ("korean", "malgun.ttf"),
        ("japanese", "YuGothM.ttc"),
        ("chinese", "msyh.ttc"),
    ] {
        if let Some(bytes) = read(file) {
            add(&mut fonts, name, bytes);
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .push(name.into());
        }
    }
    let mut semibold = Vec::new();
    if let Some(bytes) = read("seguisb.ttf") {
        add(&mut fonts, "semibold", bytes);
        semibold.push("semibold".to_string());
    }
    semibold.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name("semibold".into()), semibold);
    let icons = ["SegoeIcons.ttf", "segmdl2.ttf"].into_iter().find_map(read);
    let mut icon_family = Vec::new();
    HAS_ICONS.store(icons.is_some(), Ordering::Relaxed);
    if let Some(bytes) = icons {
        add(&mut fonts, "icons", bytes);
        icon_family.push("icons".to_string());
    }
    // Without an icon font, glyph buttons fall back to plain characters in the text font.
    icon_family.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name("icons".into()), icon_family);
    ctx.set_fonts(fonts);
}

/// Whether Segoe Fluent Icons (or MDL2) was found. Without it, glyphs use plain characters.
pub fn has_icons() -> bool {
    HAS_ICONS.load(Ordering::Relaxed)
}

pub fn semibold(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("semibold".into()))
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("icons".into()))
}

/// Give egui the look: the palette on its own widgets, pills for buttons and fields, and the
/// spacing. Called again whenever the mode or accent changes.
pub fn restyle(ctx: &egui::Context) {
    let c = colors();
    let mut visuals = if c.light {
        egui::Visuals::light()
    } else {
        egui::Visuals::dark()
    };
    visuals.panel_fill = Color32::TRANSPARENT;
    visuals.override_text_color = None;
    visuals.extreme_bg_color = c.control;
    visuals.faint_bg_color = c.row_hover;
    visuals.widgets.noninteractive.fg_stroke.color = c.text;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.hairline);
    // On a white island the hairline is too faint to show where a field is.
    let edge = if c.light {
        with_alpha(c.text, 44)
    } else {
        c.hairline
    };
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, edge);
    visuals.widgets.inactive.weak_bg_fill = c.control;
    visuals.widgets.inactive.bg_fill = c.track;
    visuals.widgets.hovered.weak_bg_fill = c.control_hover;
    visuals.widgets.hovered.bg_fill = mix(c.track, c.accent, 0.25);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, edge);
    visuals.widgets.active.bg_fill = c.accent;
    visuals.widgets.active.weak_bg_fill = c.accent;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, c.accent);
    visuals.widgets.open.weak_bg_fill = c.control_hover;
    for widget in [
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(15);
        widget.fg_stroke.color = c.text;
    }
    visuals.widgets.active.corner_radius = CornerRadius::same(15);
    visuals.widgets.active.fg_stroke.color = c.on_accent;
    visuals.selection.bg_fill = c.accent;
    visuals.selection.stroke = Stroke::new(1.0_f32, c.accent_text);
    visuals.text_cursor.stroke.color = c.accent_text;
    visuals.hyperlink_color = c.accent_text;
    visuals.slider_trailing_fill = true;
    visuals.window_fill = c.popup;
    visuals.window_stroke = Stroke::new(1.0_f32, c.hairline);
    visuals.window_corner_radius = CornerRadius::same(16);
    visuals.menu_corner_radius = CornerRadius::same(14);
    visuals.popup_shadow = egui::Shadow {
        offset: [0, 8],
        blur: 24,
        spread: 0,
        color: Color32::from_black_alpha(90),
    };
    if c.high_contrast {
        // Every control outlined, as Windows draws them in high contrast.
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.bg_stroke = Stroke::new(1.0_f32, c.text);
        }
        visuals.widgets.hovered.fg_stroke.color = c.on_accent;
        visuals.window_shadow = egui::Shadow::NONE;
        visuals.popup_shadow = egui::Shadow::NONE;
    }
    ctx.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = Vec2::new(10.0, 8.0);
        style.spacing.button_padding = Vec2::new(12.0, 7.0);
        style.spacing.interact_size.y = 30.0;
        // Selectable text would take clicks meant for the app rows it sits on.
        style.interaction.selectable_labels = false;
        use egui::TextStyle;
        style.text_styles = [
            (TextStyle::Small, FontId::proportional(11.5)),
            (TextStyle::Body, FontId::proportional(13.0)),
            (TextStyle::Button, FontId::proportional(13.0)),
            (TextStyle::Heading, semibold(17.0)),
            (TextStyle::Monospace, FontId::monospace(12.5)),
        ]
        .into();
    });
}

pub fn muted(text: impl Into<String>) -> RichText {
    RichText::new(text).color(colors().muted)
}

/// The smaller muted line under a title.
pub fn detail(text: impl Into<String>) -> RichText {
    RichText::new(text).size(11.5).color(colors().muted)
}

/// A question that needs an answer before anything else: a solid island.
pub fn dialog() -> egui::Frame {
    let c = colors();
    egui::Frame::new()
        .fill(c.popup)
        .stroke(Stroke::new(
            1.0_f32,
            if c.high_contrast { c.text } else { c.hairline },
        ))
        .corner_radius(CornerRadius::same(ISLAND_RADIUS))
        .inner_margin(egui::Margin::symmetric(22, 18))
}
