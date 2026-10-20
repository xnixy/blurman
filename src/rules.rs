//! Rules the user asked for, plus the original style of every window we touched.

use crate::mapping::{self, BlurStyle, Finish, FocusLook, Look, Rgb, SavedStyle};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Rule {
    pub process: String,
    #[serde(flatten)]
    pub look: Look,
    pub enabled: bool,
    /// How the window looks while it has focus.
    #[serde(default, skip_serializing_if = "is_same_focus")]
    pub focus: FocusLook,
    /// When not empty, only windows whose title matches one of these are frosted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub only_titles: Vec<String>,
    /// Windows whose title matches one of these are left alone.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skip_titles: Vec<String>,
}

fn is_same_focus(focus: &FocusLook) -> bool {
    *focus == FocusLook::Same
}

impl Rule {
    /// Whether a window with this title should be frosted. Windows are judged as their title
    /// changes, so a browser tab that becomes a skipped title goes back to solid.
    pub fn wants_title(&self, title: &str) -> bool {
        let any =
            |patterns: &[String]| patterns.iter().any(|pattern| title_matches(pattern, title));
        (self.only_titles.is_empty() || any(&self.only_titles)) && !any(&self.skip_titles)
    }

    pub fn filters_titles(&self) -> bool {
        !self.only_titles.is_empty() || !self.skip_titles.is_empty()
    }
}

/// Case-insensitive. A pattern with `*` must match the whole title, where `*` stands for
/// anything. A pattern without one matches anywhere in the title.
pub fn title_matches(pattern: &str, title: &str) -> bool {
    let pattern = pattern.trim().to_lowercase();
    if pattern.is_empty() {
        return false;
    }
    let title = title.to_lowercase();
    if !pattern.contains('*') {
        return title.contains(&pattern);
    }
    let parts: Vec<&str> = pattern.split('*').collect();
    let (first, rest) = parts.split_first().expect("split yields one part");
    let Some(mut remaining) = title.strip_prefix(first) else {
        return false;
    };
    let (last, middle) = rest.split_last().expect("pattern has a star");
    for part in middle {
        match remaining.find(part) {
            Some(at) => remaining = &remaining[at + part.len()..],
            None => return false,
        }
    }
    remaining.len() >= last.len() && remaining.ends_with(last)
}

/// Title patterns as the window shows them: separated by `|`.
pub fn join_titles(patterns: &[String]) -> String {
    patterns.join(" | ")
}

pub fn split_titles(text: &str) -> Vec<String> {
    text.split('|')
        .map(|part| part.trim().to_string())
        .filter(|part| !part.is_empty())
        .collect()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct Store {
    #[serde(default)]
    pub paused: bool,
    #[serde(default)]
    pub rules: Vec<Rule>,
}

impl Store {
    pub fn normalize(&mut self) {
        for rule in &mut self.rules {
            rule.process = mapping::normalize_process(&rule.process);
            rule.look = rule.look.clamped();
        }
        self.rules.retain(|rule| !rule.process.is_empty());
    }

    pub fn rule_mut(&mut self, process: &str) -> Option<&mut Rule> {
        let want = mapping::normalize_process(process);
        self.rules
            .iter_mut()
            .find(|rule| rule.process.eq_ignore_ascii_case(&want))
    }

    /// Frost an app with `look`, or switch its rule off if it is on. Returns whether it is now
    /// frosted. Used by the shortcut and the tray, which act on whatever app you were using.
    pub fn toggle_app(&mut self, process: &str, look: Look) -> bool {
        let frosted = match self.rule_mut(process) {
            Some(rule) => {
                rule.enabled = !rule.enabled;
                rule.enabled
            }
            None => {
                self.upsert(new_rule(process, look));
                true
            }
        };
        if frosted {
            self.paused = false;
        }
        frosted
    }

    pub fn rule(&self, process: &str) -> Option<&Rule> {
        let want = mapping::normalize_process(process);
        self.rules
            .iter()
            .find(|rule| rule.process.eq_ignore_ascii_case(&want))
    }

    pub fn upsert(&mut self, mut rule: Rule) {
        rule.process = mapping::normalize_process(&rule.process);
        rule.look = rule.look.clamped();
        if let Some(existing) = self
            .rules
            .iter_mut()
            .find(|item| item.process.eq_ignore_ascii_case(&rule.process))
        {
            *existing = rule;
        } else if !rule.process.is_empty() {
            self.rules.push(rule);
        }
    }

    pub fn remove(&mut self, process: &str) -> bool {
        let want = mapping::normalize_process(process);
        let before = self.rules.len();
        self.rules
            .retain(|rule| !rule.process.eq_ignore_ascii_case(&want));
        before != self.rules.len()
    }

    pub fn clear(&mut self) {
        self.rules.clear();
        self.paused = false;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedWindow {
    pub hwnd: isize,
    pub pid: u32,
    pub process_start: u64,
    pub process: String,
    pub original_exstyle: i64,
    pub original_layered: bool,
    pub original_alpha: u8,
}

impl PersistedWindow {
    pub fn new(
        hwnd: isize,
        pid: u32,
        process_start: u64,
        process: String,
        saved: &SavedStyle,
    ) -> Self {
        Self {
            hwnd,
            pid,
            process_start,
            process,
            original_exstyle: saved.exstyle,
            original_layered: saved.was_layered,
            original_alpha: saved.alpha,
        }
    }

    pub fn saved_style(&self) -> SavedStyle {
        SavedStyle {
            exstyle: self.original_exstyle,
            was_layered: self.original_layered,
            alpha: self.original_alpha,
        }
    }
}

/// Window behavior. Start with Windows lives in the registry, not here.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Settings {
    pub close_to_tray: bool,
    /// Ask GitHub once a day whether a newer Blurman is out.
    pub check_updates: bool,
    pub hotkeys: Hotkeys,
    pub auto_pause: AutoPause,
    pub presets: Vec<Preset>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            close_to_tray: false,
            check_updates: true,
            hotkeys: Hotkeys::default(),
            auto_pause: AutoPause::default(),
            presets: default_presets(),
        }
    }
}

/// Global shortcuts, written like "Ctrl+Alt+B". An empty one is switched off.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Hotkeys {
    pub enabled: bool,
    pub pause: String,
    pub frost_front: String,
    pub peek: String,
    pub more_glass: String,
    pub less_glass: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            enabled: true,
            pause: "Ctrl+Alt+B".into(),
            frost_front: "Ctrl+Alt+F".into(),
            peek: "Ctrl+Alt+Z".into(),
            more_glass: "Ctrl+Alt+PageUp".into(),
            less_glass: "Ctrl+Alt+PageDown".into(),
        }
    }
}

/// What each shortcut does. The discriminant is the id given to RegisterHotKey.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyAction {
    Pause = 1,
    FrostFront = 2,
    Peek = 3,
    MoreGlass = 4,
    LessGlass = 5,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 5] = [
        HotkeyAction::Pause,
        HotkeyAction::FrostFront,
        HotkeyAction::Peek,
        HotkeyAction::MoreGlass,
        HotkeyAction::LessGlass,
    ];

    pub fn from_id(id: usize) -> Option<Self> {
        Self::ALL.into_iter().find(|action| *action as usize == id)
    }

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::Pause => "Pause or resume",
            HotkeyAction::FrostFront => "Frost the app in front",
            HotkeyAction::Peek => "Hold to see apps solid",
            HotkeyAction::MoreGlass => "More see-through",
            HotkeyAction::LessGlass => "Less see-through",
        }
    }
}

impl Hotkeys {
    pub fn get(&self, action: HotkeyAction) -> &str {
        match action {
            HotkeyAction::Pause => &self.pause,
            HotkeyAction::FrostFront => &self.frost_front,
            HotkeyAction::Peek => &self.peek,
            HotkeyAction::MoreGlass => &self.more_glass,
            HotkeyAction::LessGlass => &self.less_glass,
        }
    }

    pub fn set(&mut self, action: HotkeyAction, combo: String) {
        let slot = match action {
            HotkeyAction::Pause => &mut self.pause,
            HotkeyAction::FrostFront => &mut self.frost_front,
            HotkeyAction::Peek => &mut self.peek,
            HotkeyAction::MoreGlass => &mut self.more_glass,
            HotkeyAction::LessGlass => &mut self.less_glass,
        };
        *slot = combo;
    }
}

/// Put every app back on its own while one of these holds, without touching the saved rules.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AutoPause {
    pub battery_saver: bool,
    /// Do not disturb, or presentation mode.
    pub quiet: bool,
    pub schedule: bool,
    /// Local time, "HH:MM". A window that crosses midnight, like 22:00 to 07:00, is fine.
    pub from: String,
    pub until: String,
}

impl Default for AutoPause {
    fn default() -> Self {
        Self {
            battery_saver: false,
            quiet: false,
            schedule: false,
            from: "22:00".into(),
            until: "07:00".into(),
        }
    }
}

impl AutoPause {
    pub fn any(&self) -> bool {
        self.battery_saver || self.quiet || self.schedule
    }

    /// Whether `now`, in minutes after midnight, falls inside the schedule.
    pub fn in_schedule(&self, now: u16) -> bool {
        let (Some(from), Some(until)) = (parse_clock(&self.from), parse_clock(&self.until)) else {
            return false;
        };
        if from <= until {
            (from..until).contains(&now)
        } else {
            now >= from || now < until
        }
    }
}

/// "7:05" or "07:05" to minutes after midnight.
pub fn parse_clock(text: &str) -> Option<u16> {
    let (hours, minutes) = text.trim().split_once(':')?;
    let hours: u16 = hours.trim().parse().ok()?;
    let minutes: u16 = minutes.trim().parse().ok()?;
    (hours < 24 && minutes < 60).then_some(hours * 60 + minutes)
}

/// A named look you can put on any app with one click.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Preset {
    pub name: String,
    #[serde(flatten)]
    pub look: Look,
}

pub fn default_presets() -> Vec<Preset> {
    let preset = |name: &str, transparency, blur, style, finish| Preset {
        name: name.into(),
        look: Look {
            transparency,
            blur,
            style,
            finish,
        },
    };
    vec![
        preset("Subtle", 20, 30, BlurStyle::Frost, Finish::default()),
        preset("Heavy glass", 55, 90, BlurStyle::Frost, Finish::default()),
        preset(
            "Vibrant",
            40,
            60,
            BlurStyle::Frost,
            Finish {
                saturation: 170,
                grain: 25,
                ..Finish::default()
            },
        ),
        preset(
            "Smoke",
            45,
            70,
            BlurStyle::Frost,
            Finish {
                tint: Rgb(0x10, 0x10, 0x14),
                tint_amount: 35,
                saturation: 80,
                grain: 15,
                ..Finish::default()
            },
        ),
        preset("Milky", 40, 60, BlurStyle::Acrylic, Finish::default()),
    ]
}

impl Settings {
    pub fn preset(&self, name: &str) -> Option<&Preset> {
        self.presets
            .iter()
            .find(|preset| preset.name.eq_ignore_ascii_case(name.trim()))
    }

    /// Add a preset, or replace the one with the same name.
    pub fn save_preset(&mut self, preset: Preset) {
        match self
            .presets
            .iter_mut()
            .find(|item| item.name.eq_ignore_ascii_case(&preset.name))
        {
            Some(existing) => *existing = preset,
            None => self.presets.push(preset),
        }
    }
}

/// What `blurman export` writes and `blurman import` reads.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Bundle {
    pub rules: Vec<Rule>,
    pub presets: Vec<Preset>,
}

pub fn load_settings() -> Settings {
    fs::read_to_string(settings_path())
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

pub fn save_settings(settings: &Settings) -> Result<(), String> {
    let text = serde_json::to_string_pretty(settings).map_err(|err| err.to_string())?;
    write_atomic(&settings_path(), &text)
}

pub fn settings_path() -> PathBuf {
    app_dir().join("settings.json")
}

pub fn app_dir() -> PathBuf {
    let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join(crate::ipc::scoped("Blurman"))
}

pub fn rules_path() -> PathBuf {
    app_dir().join("rules.json")
}

pub fn state_path() -> PathBuf {
    app_dir().join("state.json")
}

/// Set when an unreadable rules file was moved aside, until someone reports it.
static RECOVERED: Mutex<Option<String>> = Mutex::new(None);

pub fn load() -> Store {
    load_from(&rules_path())
}

fn load_from(path: &Path) -> Store {
    let mut store = match fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(store) => store,
            Err(err) => {
                set_aside(path, &err.to_string());
                Store::default()
            }
        },
        Err(_) => Store::default(),
    };
    store.normalize();
    store
}

/// Keep a rules file that does not parse, so the next save does not overwrite the user's rules.
fn set_aside(path: &Path, reason: &str) {
    let backup = path.with_extension("json.bad");
    if fs::rename(path, &backup).is_err() {
        return;
    }
    let note = format!(
        "rules.json could not be read ({reason}). It was kept as {}.",
        backup.display()
    );
    if let Ok(mut slot) = RECOVERED.lock() {
        *slot = Some(note);
    }
}

/// The message about a rules file moved aside, once.
pub fn take_recovery_note() -> Option<String> {
    RECOVERED.lock().ok().and_then(|mut slot| slot.take())
}

pub fn save(store: &Store) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|err| err.to_string())?;
    write_atomic(&rules_path(), &text)
}

pub fn load_state() -> Vec<PersistedWindow> {
    match fs::read_to_string(state_path()) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub fn save_state(windows: &[PersistedWindow]) -> Result<(), String> {
    let text = serde_json::to_string_pretty(windows).map_err(|err| err.to_string())?;
    write_atomic(&state_path(), &text)
}

/// Readers never see a half-written file, so a reload mid-save cannot wipe the rules.
fn write_atomic(path: &Path, text: &str) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|err| err.to_string())?;
    }
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, text).map_err(|err| err.to_string())?;
    fs::rename(&temp, path).map_err(|err| err.to_string())
}

pub fn new_rule(process: &str, look: Look) -> Rule {
    Rule {
        process: mapping::normalize_process(process),
        look: look.clamped(),
        enabled: true,
        focus: FocusLook::Same,
        only_titles: Vec::new(),
        skip_titles: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn look(transparency: u8, blur: u8, style: BlurStyle) -> Look {
        Look {
            transparency,
            blur,
            style,
            finish: Finish::default(),
        }
    }

    #[test]
    fn upsert_matches_names_case_insensitively() {
        let mut store = Store::default();
        store.upsert(new_rule("Chrome", look(30, 40, BlurStyle::Frost)));
        store.upsert(new_rule("chrome.exe", look(50, 60, BlurStyle::Acrylic)));
        assert_eq!(store.rules.len(), 1);
        assert_eq!(store.rules[0].look.transparency, 50);
        assert_eq!(store.rules[0].look.blur, 60);
        assert_eq!(store.rules[0].look.style, BlurStyle::Acrylic);
        assert!(store.remove("CHROME.EXE"));
        assert!(store.rules.is_empty());
    }

    #[test]
    fn toggling_an_app_adds_then_switches_off_then_on() {
        let mut store = Store {
            paused: true,
            ..Store::default()
        };
        assert!(store.toggle_app("notepad", look(30, 40, BlurStyle::Frost)));
        assert!(!store.paused);
        assert!(!store.toggle_app("notepad.exe", Look::default()));
        assert!(!store.rule("notepad").unwrap().enabled);
        assert!(store.toggle_app("NOTEPAD.EXE", Look::default()));
        assert_eq!(store.rules.len(), 1);
        assert_eq!(store.rule("notepad").unwrap().look.transparency, 30);
    }

    #[test]
    fn title_patterns_match_like_people_expect() {
        assert!(title_matches("youtube", "Cats - YouTube - Google Chrome"));
        assert!(!title_matches("youtube", "Inbox - Gmail"));
        assert!(title_matches(
            "*- youtube -*",
            "Cats - YouTube - Google Chrome"
        ));
        assert!(title_matches("inbox*", "Inbox - Gmail"));
        assert!(!title_matches("gmail*", "Inbox - Gmail"));
        assert!(title_matches("*gmail", "Inbox - Gmail"));
        assert!(title_matches("a*b*b", "abb"));
        assert!(!title_matches("a*b*b", "ab"));
        assert!(!title_matches("  ", "anything"));

        let mut rule = new_rule("chrome", Look::default());
        assert!(rule.wants_title("anything"));
        rule.skip_titles = split_titles("YouTube | Netflix");
        assert_eq!(rule.skip_titles, vec!["YouTube", "Netflix"]);
        assert!(!rule.wants_title("Cats - YouTube"));
        assert!(rule.wants_title("Inbox"));
        rule.only_titles = vec!["Visual Studio Code".into()];
        assert!(!rule.wants_title("Inbox"));
        assert!(rule.wants_title("main.rs - Visual Studio Code"));
    }

    #[test]
    fn schedules_can_cross_midnight() {
        let night = AutoPause {
            schedule: true,
            ..AutoPause::default()
        };
        assert!(night.in_schedule(parse_clock("23:30").unwrap()));
        assert!(night.in_schedule(parse_clock("6:59").unwrap()));
        assert!(!night.in_schedule(parse_clock("07:00").unwrap()));
        assert!(!night.in_schedule(parse_clock("12:00").unwrap()));
        let day = AutoPause {
            from: "09:00".into(),
            until: "17:30".into(),
            ..night
        };
        assert!(day.in_schedule(parse_clock("12:00").unwrap()));
        assert!(!day.in_schedule(parse_clock("17:30").unwrap()));
        assert_eq!(parse_clock("24:00"), None);
        assert_eq!(parse_clock("noon"), None);
    }

    #[test]
    fn old_settings_files_get_the_new_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"close_to_tray":true}"#).unwrap();
        assert!(settings.close_to_tray);
        assert!(settings.check_updates);
        assert!(settings.hotkeys.enabled);
        assert!(!settings.presets.is_empty());
        assert!(settings.preset("subtle").is_some());
        let roundtrip: Settings =
            serde_json::from_str(&serde_json::to_string(&settings).unwrap()).unwrap();
        assert_eq!(roundtrip, settings);
    }

    #[test]
    fn rules_keep_their_finish_and_filters_through_a_save() {
        let mut rule = new_rule("code", look(30, 40, BlurStyle::Frost));
        rule.look.finish.tint = Rgb(1, 2, 3);
        rule.look.finish.tint_amount = 20;
        rule.focus = FocusLook::Solid;
        rule.skip_titles = vec!["Settings".into()];
        let text = serde_json::to_string(&rule).unwrap();
        assert!(text.contains("\"transparency\":30"), "{text}");
        assert!(text.contains("\"focus\":\"solid\""), "{text}");
        let back: Rule = serde_json::from_str(&text).unwrap();
        assert_eq!(back, rule);
        // A plain rule stays as small as it was before finishes existed.
        let plain = serde_json::to_string(&new_rule("notepad", Look::default())).unwrap();
        assert!(
            !plain.contains("finish") && !plain.contains("focus"),
            "{plain}"
        );
    }

    #[test]
    fn stored_values_are_clamped() {
        let mut store: Store = serde_json::from_str(
            r#"{"rules":[{"process":"notepad","transparency":250,"blur":0,"enabled":true},
                         {"process":"  ","transparency":30,"blur":40,"enabled":true}]}"#,
        )
        .unwrap();
        store.normalize();
        assert_eq!(store.rules.len(), 1);
        assert_eq!(store.rules[0].process, "notepad.exe");
        assert_eq!(store.rules[0].look.transparency, mapping::TRANSPARENCY_MAX);
        assert_eq!(store.rules[0].look.blur, mapping::BLUR_MIN);
        assert_eq!(store.rules[0].look.style, BlurStyle::Frost);
        assert!(!store.paused);
    }

    #[test]
    fn an_unreadable_rules_file_is_kept_aside_instead_of_lost() {
        let dir = std::env::temp_dir().join(format!("blurman-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rules.json");
        fs::write(&path, r#"{"rules":[{"process":"notepad.exe",}]}"#).unwrap();
        let store = load_from(&path);
        assert!(store.rules.is_empty());
        assert!(!path.exists());
        let kept = dir.join("rules.json.bad");
        assert!(fs::read_to_string(&kept).unwrap().contains("notepad.exe"));
        assert!(take_recovery_note().is_some_and(|note| note.contains("rules.json.bad")));
        assert!(take_recovery_note().is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn acrylic_style_roundtrips_and_old_rules_stay_frost() {
        let mut store: Store = serde_json::from_str(
            r#"{"rules":[
                {"process":"notepad.exe","transparency":30,"blur":40,"style":"acrylic","enabled":true},
                {"process":"chrome.exe","transparency":30,"blur":40,"enabled":true}
            ]}"#,
        )
        .unwrap();
        store.normalize();
        assert_eq!(store.rules[0].look.style, BlurStyle::Acrylic);
        assert_eq!(store.rules[1].look.style, BlurStyle::Frost);
        let text = serde_json::to_string(&store.rules[0]).unwrap();
        assert!(text.contains("\"style\":\"acrylic\""));
    }
}
