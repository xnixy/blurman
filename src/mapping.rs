//! Slider ranges and the decision of how a window is put back.

/// See-through percent. 10 keeps the app readable, 70 is the most glass we allow.
pub const TRANSPARENCY_MIN: u8 = 10;
pub const TRANSPARENCY_MAX: u8 = 70;
pub const TRANSPARENCY_DEFAULT: u8 = 30;

/// Blur strength. Never zero: a rule that is on always blurs what shows through.
pub const BLUR_MIN: u8 = 1;
pub const BLUR_MAX: u8 = 100;
pub const BLUR_DEFAULT: u8 = 40;

/// How the glass is drawn. Frost is the adjustable blur. Acrylic is Windows' milky system glass,
/// where the same slider sets how milky it is. Missing values in old rules files stay Frost.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum BlurStyle {
    #[default]
    Frost,
    Acrylic,
}

impl std::fmt::Display for BlurStyle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            BlurStyle::Frost => "frost",
            BlurStyle::Acrylic => "acrylic",
        })
    }
}

/// How much color is laid over frost, in percent. Past this the glass stops reading as glass.
pub const TINT_MAX: u8 = 80;
/// Color strength of what shows through, in percent. 100 leaves it as it is.
pub const SATURATION_MIN: u8 = 0;
pub const SATURATION_MAX: u8 = 200;
pub const SATURATION_DEFAULT: u8 = 100;
pub const GRAIN_MAX: u8 = 100;
/// Contrast added to what shows through, in percent. 0 leaves it as it is.
pub const CONTRAST_MAX: u8 = 100;
/// How far what shows through is lifted toward milky white, in percent, keeping its color. This
/// is what keeps the glass behind an app in light mode bright instead of grey.
pub const MILK_MAX: u8 = 100;

/// The tint used when a rule never picked one, and the color acrylic always used before.
pub const TINT_DEFAULT: Rgb = Rgb(0x18, 0x18, 0x18);

/// An sRGB color, saved as "#rrggbb".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

impl Rgb {
    /// "#3a7bd5", "3a7bd5", and "#3A7BD5" all parse.
    pub fn parse(text: &str) -> Option<Self> {
        let hex = text.trim().trim_start_matches('#');
        if hex.len() != 6 || !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        let byte = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
        Some(Self(byte(0)?, byte(2)?, byte(4)?))
    }

    pub fn hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.0, self.1, self.2)
    }
}

impl serde::Serialize for Rgb {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.hex())
    }
}

impl<'de> serde::Deserialize<'de> for Rgb {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Rgb::parse(&text).ok_or_else(|| serde::de::Error::custom(format!("not a color: {text}")))
    }
}

/// Where the tint color comes from. Theme and Accent follow Windows as it changes.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum TintSource {
    #[default]
    Custom,
    /// Dark or light, matching the Windows app mode.
    Theme,
    /// The Windows accent color.
    Accent,
}

/// Color, saturation, and grain on top of the blur. The defaults change nothing, so rules saved
/// before these existed look the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Finish {
    pub tint: Rgb,
    pub tint_source: TintSource,
    /// Percent of tint laid over frost. Acrylic always uses the color, and its slider sets how much.
    pub tint_amount: u8,
    pub saturation: u8,
    pub grain: u8,
    pub contrast: u8,
    pub milk: u8,
}

impl Default for Finish {
    fn default() -> Self {
        Self {
            tint: TINT_DEFAULT,
            tint_source: TintSource::Custom,
            tint_amount: 0,
            saturation: SATURATION_DEFAULT,
            grain: 0,
            contrast: 0,
            milk: 0,
        }
    }
}

impl Finish {
    pub fn clamped(self) -> Self {
        Self {
            tint_amount: self.tint_amount.min(TINT_MAX),
            saturation: self.saturation.clamp(SATURATION_MIN, SATURATION_MAX),
            grain: self.grain.min(GRAIN_MAX),
            contrast: self.contrast.min(CONTRAST_MAX),
            milk: self.milk.min(MILK_MAX),
            ..self
        }
    }
}

/// Everything about how the glass looks. Presets save one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Look {
    pub transparency: u8,
    pub blur: u8,
    /// Frost or acrylic. Omitted in rules saved before the choice existed, which means frost.
    #[serde(default)]
    pub style: BlurStyle,
    #[serde(default, skip_serializing_if = "is_default_finish")]
    pub finish: Finish,
}

fn is_default_finish(finish: &Finish) -> bool {
    *finish == Finish::default()
}

impl Default for Look {
    fn default() -> Self {
        Self {
            transparency: TRANSPARENCY_DEFAULT,
            blur: BLUR_DEFAULT,
            style: BlurStyle::Frost,
            finish: Finish::default(),
        }
    }
}

impl Look {
    pub fn clamped(self) -> Self {
        Self {
            transparency: clamp_transparency(self.transparency),
            blur: clamp_blur(self.blur),
            finish: self.finish.clamped(),
            ..self
        }
    }
}

/// How a frosted window looks while it is the one you are using. Same keeps the rule's look.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Default,
    serde::Serialize,
    serde::Deserialize,
    clap::ValueEnum,
)]
#[serde(rename_all = "lowercase")]
pub enum FocusLook {
    #[default]
    Same,
    /// Half as see-through while focused.
    Lighter,
    /// Fully solid while focused, frosted again once you move on.
    Solid,
}

/// The window alpha for a frosted window right now. Peeking shows every app solid.
pub fn window_alpha(transparency: u8, focus: FocusLook, focused: bool, peeking: bool) -> u8 {
    if peeking {
        return 255;
    }
    if !focused {
        return transparency_to_alpha(transparency);
    }
    match focus {
        FocusLook::Same => transparency_to_alpha(transparency),
        FocusLook::Lighter => alpha_for_percent(clamp_transparency(transparency) / 2),
        FocusLook::Solid => 255,
    }
}

const BLUR_RADIUS_MIN: f32 = 4.0;
const BLUR_RADIUS_MAX: f32 = 48.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SavedStyle {
    pub exstyle: i64,
    pub was_layered: bool,
    pub alpha: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreAction {
    /// The window was already layered. Put its previous alpha back and leave the style bit.
    KeepLayered { alpha: u8 },
    /// We added the layered bit. Take it off and ask the window to repaint.
    ClearLayered,
}

pub fn clamp_transparency(value: u8) -> u8 {
    value.clamp(TRANSPARENCY_MIN, TRANSPARENCY_MAX)
}

pub fn clamp_blur(value: u8) -> u8 {
    value.clamp(BLUR_MIN, BLUR_MAX)
}

/// Higher transparency means a lower window alpha (more of the glass shows through).
pub fn transparency_to_alpha(transparency: u8) -> u8 {
    alpha_for_percent(clamp_transparency(transparency))
}

fn alpha_for_percent(percent: u8) -> u8 {
    let t = percent.min(100) as f32 / 100.0;
    (255.0 * (1.0 - t)).round() as u8
}

/// Maps the blur slider onto the Gaussian standard deviation in DIPs.
/// The visible radius is about three times this value, so the slider covers
/// roughly 12px to 144px of frost while the effect itself stays in a stable range.
pub fn blur_strength_to_radius(strength: u8) -> f32 {
    let s = clamp_blur(strength) as f32;
    let span = (BLUR_MAX - BLUR_MIN) as f32;
    BLUR_RADIUS_MIN + (s - BLUR_MIN as f32) * (BLUR_RADIUS_MAX - BLUR_RADIUS_MIN) / span
}

/// Fallback acrylic has no radius. The slider instead picks how milky the tint is.
pub fn blur_strength_to_tint_alpha(strength: u8) -> u8 {
    let s = clamp_blur(strength) as f32;
    let span = (BLUR_MAX - BLUR_MIN) as f32;
    let alpha = 28.0 + (s - BLUR_MIN as f32) * (200.0 - 28.0) / span;
    alpha.round() as u8
}

pub fn restore_action(saved: &SavedStyle) -> RestoreAction {
    if saved.was_layered {
        RestoreAction::KeepLayered { alpha: saved.alpha }
    } else {
        RestoreAction::ClearLayered
    }
}

/// "chrome", "chrome.exe", and a full path all become "chrome.exe".
pub fn normalize_process(name: &str) -> String {
    let trimmed = name.trim().trim_matches('"');
    let file = trimmed.rsplit(['\\', '/']).next().unwrap_or(trimmed).trim();
    if file.is_empty() {
        return String::new();
    }
    if file.to_ascii_lowercase().ends_with(".exe") {
        file.to_string()
    } else {
        format!("{file}.exe")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparency_stays_visible_and_orders_correctly() {
        let solid = transparency_to_alpha(TRANSPARENCY_MIN);
        let glassy = transparency_to_alpha(TRANSPARENCY_MAX);
        assert!(solid > glassy);
        assert!(glassy >= 76);
        assert!(solid < 255);
        assert_eq!(transparency_to_alpha(0), solid);
        assert_eq!(transparency_to_alpha(100), glassy);
    }

    #[test]
    fn blur_slider_never_reaches_a_sharp_backdrop() {
        let low = blur_strength_to_radius(BLUR_MIN);
        let high = blur_strength_to_radius(BLUR_MAX);
        assert!((low - 4.0).abs() < 0.01);
        assert!((high - 48.0).abs() < 0.01);
        assert!(low > 0.0);
        assert!(high > low);
        assert!(blur_strength_to_tint_alpha(BLUR_MIN) < blur_strength_to_tint_alpha(BLUR_MAX));
    }

    #[test]
    fn restore_respects_windows_that_were_already_layered() {
        let layered = SavedStyle {
            exstyle: 0x0008_0000,
            was_layered: true,
            alpha: 200,
        };
        assert_eq!(
            restore_action(&layered),
            RestoreAction::KeepLayered { alpha: 200 }
        );
        let plain = SavedStyle {
            exstyle: 0x0000_0100,
            was_layered: false,
            alpha: 255,
        };
        assert_eq!(restore_action(&plain), RestoreAction::ClearLayered);
    }

    #[test]
    fn colors_parse_and_print_as_hex() {
        assert_eq!(Rgb::parse("#3a7bd5"), Some(Rgb(0x3a, 0x7b, 0xd5)));
        assert_eq!(Rgb::parse(" 3A7BD5 "), Some(Rgb(0x3a, 0x7b, 0xd5)));
        assert_eq!(Rgb::parse("#3a7bd"), None);
        assert_eq!(Rgb::parse("#zzzzzz"), None);
        assert_eq!(Rgb(0x3a, 0x7b, 0xd5).hex(), "#3a7bd5");
        let text = serde_json::to_string(&Rgb(1, 2, 3)).unwrap();
        assert_eq!(text, "\"#010203\"");
        assert_eq!(serde_json::from_str::<Rgb>(&text).unwrap(), Rgb(1, 2, 3));
    }

    #[test]
    fn focus_and_peek_only_ever_make_a_window_more_solid() {
        let base = transparency_to_alpha(40);
        assert_eq!(window_alpha(40, FocusLook::Same, true, false), base);
        assert_eq!(window_alpha(40, FocusLook::Solid, false, false), base);
        assert_eq!(window_alpha(40, FocusLook::Solid, true, false), 255);
        let lighter = window_alpha(40, FocusLook::Lighter, true, false);
        assert!(lighter > base && lighter < 255);
        assert_eq!(window_alpha(40, FocusLook::Same, false, true), 255);
    }

    #[test]
    fn finish_values_are_clamped() {
        let finish = Finish {
            tint_amount: 200,
            saturation: 250,
            grain: 120,
            ..Finish::default()
        }
        .clamped();
        assert_eq!(finish.tint_amount, TINT_MAX);
        assert_eq!(finish.saturation, SATURATION_MAX);
        assert_eq!(finish.grain, GRAIN_MAX);
    }

    #[test]
    fn process_names_normalize() {
        assert_eq!(normalize_process("chrome"), "chrome.exe");
        assert_eq!(
            normalize_process("C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe"),
            "chrome.exe"
        );
        assert_eq!(normalize_process("notepad.EXE"), "notepad.EXE");
    }
}
