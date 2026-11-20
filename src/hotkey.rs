//! Global shortcuts: "Ctrl+Alt+F" to and from the modifier and virtual-key pair Windows wants.

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL,
    MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub vk: u32,
}

/// Key names as written in settings.json, with their virtual-key codes.
const NAMED: &[(&str, u32)] = &[
    ("Space", 0x20),
    ("PageUp", 0x21),
    ("PageDown", 0x22),
    ("End", 0x23),
    ("Home", 0x24),
    ("Left", 0x25),
    ("Up", 0x26),
    ("Right", 0x27),
    ("Down", 0x28),
    ("Insert", 0x2d),
    ("Delete", 0x2e),
    ("Pause", 0x13),
    (";", 0xba),
    ("=", 0xbb),
    (",", 0xbc),
    ("-", 0xbd),
    (".", 0xbe),
    ("/", 0xbf),
    ("`", 0xc0),
    ("[", 0xdb),
    ("\\", 0xdc),
    ("]", 0xdd),
    ("'", 0xde),
];

fn key_name(vk: u32) -> Option<String> {
    match vk {
        0x30..=0x39 | 0x41..=0x5a => Some(char::from_u32(vk)?.to_string()),
        0x60..=0x69 => Some(format!("Num{}", vk - 0x60)),
        0x70..=0x87 => Some(format!("F{}", vk - 0x6f)),
        _ => NAMED
            .iter()
            .find(|(_, code)| *code == vk)
            .map(|(name, _)| name.to_string()),
    }
}

fn key_code(name: &str) -> Option<u32> {
    let upper = name.to_ascii_uppercase();
    let mut chars = upper.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(ch as u32);
        }
    }
    if let Some(number) = upper.strip_prefix("NUM") {
        if let Ok(digit @ 0..=9) = number.parse::<u32>() {
            return Some(0x60 + digit);
        }
    }
    if let Some(number) = upper.strip_prefix('F') {
        if let Ok(n @ 1..=24) = number.parse::<u32>() {
            return Some(0x6f + n);
        }
    }
    NAMED
        .iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, code)| *code)
}

impl Combo {
    /// None for an empty string, which means the shortcut is off.
    pub fn parse(text: &str) -> Result<Option<Combo>, String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(None);
        }
        let mut combo = Combo {
            ctrl: false,
            alt: false,
            shift: false,
            win: false,
            vk: 0,
        };
        // "+" itself is written as "=" so the separator stays unambiguous.
        for part in text.split('+').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => combo.ctrl = true,
                "alt" => combo.alt = true,
                "shift" => combo.shift = true,
                "win" | "windows" => combo.win = true,
                _ if combo.vk == 0 => {
                    combo.vk = key_code(part).ok_or_else(|| format!("Unknown key \"{part}\"."))?
                }
                _ => return Err(format!("\"{text}\" has more than one key.")),
            }
        }
        if combo.vk == 0 {
            return Err(format!("\"{text}\" has no key, only modifiers."));
        }
        if !(combo.ctrl || combo.alt || combo.win) && !(0x70..=0x87).contains(&combo.vk) {
            return Err(format!(
                "\"{text}\" needs Ctrl, Alt, or Win, or it would block typing."
            ));
        }
        Ok(Some(combo))
    }

    pub fn text(&self) -> String {
        // Windows writes the Windows key first, as in Win+Shift+S.
        let mut parts = Vec::new();
        if self.win {
            parts.push("Win".to_string());
        }
        if self.ctrl {
            parts.push("Ctrl".to_string());
        }
        if self.alt {
            parts.push("Alt".to_string());
        }
        if self.shift {
            parts.push("Shift".to_string());
        }
        parts.push(key_name(self.vk).unwrap_or_else(|| format!("0x{:02x}", self.vk)));
        parts.join("+")
    }

    fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut mods = MOD_NOREPEAT;
        if self.ctrl {
            mods |= MOD_CONTROL;
        }
        if self.alt {
            mods |= MOD_ALT;
        }
        if self.shift {
            mods |= MOD_SHIFT;
        }
        if self.win {
            mods |= MOD_WIN;
        }
        mods
    }

    pub fn register(&self, hwnd: HWND, id: i32) -> bool {
        unsafe { RegisterHotKey(Some(hwnd), id, self.modifiers(), self.vk) }.is_ok()
    }

    /// Whether the key itself, not the modifiers, is still held down.
    pub fn key_down(&self) -> bool {
        (unsafe { GetAsyncKeyState(self.vk as i32) } as u16 & 0x8000) != 0
    }
}

pub fn unregister(hwnd: HWND, id: i32) {
    unsafe {
        let _ = UnregisterHotKey(Some(hwnd), id);
    }
}

/// The settings text for a key pressed in the Blurman window, from its virtual-key code and the
/// modifiers held, or None for a key Blurman cannot use as a shortcut (a lone modifier, a key
/// without Ctrl, Alt or Win). Windows gives number pad digits their own codes, so they stay
/// number pad keys.
pub fn recorded(vk: u32, ctrl: bool, alt: bool, shift: bool, win: bool) -> Option<String> {
    let text = Combo {
        ctrl,
        alt,
        shift,
        win,
        vk,
    }
    .text();
    Combo::parse(&text).ok().flatten().map(|combo| combo.text())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcuts_parse_and_print_the_same_way() {
        let combo = Combo::parse("ctrl + alt + f").unwrap().unwrap();
        assert!(combo.ctrl && combo.alt && !combo.shift && !combo.win);
        assert_eq!(combo.vk, 'F' as u32);
        assert_eq!(combo.text(), "Ctrl+Alt+F");
        for text in [
            "Ctrl+Alt+PageUp",
            "Win+Shift+F12",
            "Alt+Num5",
            "Ctrl+=",
            "F9",
        ] {
            assert_eq!(Combo::parse(text).unwrap().unwrap().text(), text);
        }
        assert_eq!(Combo::parse("  ").unwrap(), None);
    }

    #[test]
    fn shortcuts_that_would_block_typing_are_refused() {
        assert!(Combo::parse("B").is_err());
        assert!(Combo::parse("Shift+B").is_err());
        assert!(Combo::parse("Ctrl+Alt").is_err());
        assert!(Combo::parse("Ctrl+A+B").is_err());
        assert!(Combo::parse("Ctrl+Banana").is_err());
    }

    #[test]
    fn keys_pressed_in_the_window_become_shortcuts() {
        let ctrl_alt = |vk| recorded(vk, true, true, false, false);
        assert_eq!(ctrl_alt(0x46).as_deref(), Some("Ctrl+Alt+F"));
        assert_eq!(ctrl_alt(0x22).as_deref(), Some("Ctrl+Alt+PageDown"));
        assert_eq!(recorded(0x46, false, false, false, false), None);
        assert_eq!(recorded(0x77, false, false, false, false).as_deref(), Some("F8"));
        assert_eq!(recorded(0x46, false, false, false, true).as_deref(), Some("Win+F"));
        // A lone modifier is not a shortcut yet.
        assert_eq!(recorded(0x11, true, false, false, false), None);
        // A digit on the number pad is not the digit on the top row.
        assert_eq!(ctrl_alt(0x35).as_deref(), Some("Ctrl+Alt+5"));
        assert_eq!(ctrl_alt(0x65).as_deref(), Some("Ctrl+Alt+Num5"));
    }
}
