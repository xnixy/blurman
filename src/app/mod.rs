//! The Blurman window: a sidebar of pages over the frosted backdrop.

mod apps;
mod pages;
mod widgets;

use crate::autostart;
use crate::ipc;
use crate::mapping::{FocusLook, Look};
use crate::rules::{self, HotkeyAction, Rule, Settings, Store};
use crate::shared::{Shared, Tweak};
use crate::system;
use crate::target::{self, AppGroup};
use crate::theme::{self, ACCENT, MUTED, OK, TEXT, WARN};
use crate::tray;
use egui::{Align, Layout, RichText, Sense, Ui};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use widgets::{IconCache, NavIcon};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_BORDER_COLOR, DWMWA_CAPTION_COLOR, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_TEXT_COLOR,
    DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
    DWMWINDOWATTRIBUTE,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::WM_NCACTIVATE;

const RESCAN: Duration = Duration::from_secs(1);
/// How long a slider may sit still, while the button is held, before the rules file is written.
const SAVE_AFTER: Duration = Duration::from_millis(80);
/// How long a confirmation stays up, the last part of it fading.
const TOAST_FOR: Duration = Duration::from_millis(2600);
const SIDEBAR_WIDTH: f32 = 212.0;
const WINDOW_SIZE: [f32; 2] = [1080.0, 720.0];

/// Show the window, and while Blurman sits in the tray, close it for real. eframe spins a CPU
/// core when its window is merely hidden, so the tray waits on plain Win32 messages instead.
pub fn run(shared: Arc<Shared>, startup: bool) -> Result<(), String> {
    shared
        .ui_thread
        .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    tray::install_handlers(shared.clone());
    let result = window_and_tray(&shared, startup);
    tray::remove();
    result
}

fn window_and_tray(shared: &Arc<Shared>, startup: bool) -> Result<(), String> {
    // A startup launch lives in the tray, and closing its window goes back there.
    let mut in_tray = startup && tray::sync(true, &rules::load()).is_ok();
    loop {
        if in_tray && !tray::wait_for_open() {
            return Ok(());
        }
        open_window(shared, startup)?;
        if !shared.to_tray.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        in_tray = true;
    }
}

fn open_window(shared: &Arc<Shared>, tray_session: bool) -> Result<(), String> {
    let settings = rules::load_settings();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Blurman")
            .with_inner_size(WINDOW_SIZE)
            .with_min_inner_size([880.0, 580.0])
            .with_transparent(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    let app_shared = shared.clone();
    // eframe keeps window size and position in a folder named after the app, so a test
    // profile gets its own name and leaves yours alone.
    let result = eframe::run_native(
        &ipc::scoped("Blurman"),
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            app_shared.set_ctx(Some(cc.egui_ctx.clone()));
            let mut glass = false;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    app_shared
                        .main_window
                        .store(win32.hwnd.get(), Ordering::SeqCst);
                    glass = frost_window(HWND(win32.hwnd.get() as *mut _));
                }
            }
            #[allow(unused_mut)]
            let mut app = BlurmanApp::new(app_shared, settings, tray_session, glass);
            #[cfg(debug_assertions)]
            app.preview_from_env();
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| err.to_string());
    shared.main_window.store(0, Ordering::SeqCst);
    shared.set_ctx(None);
    result
}

/// Give the window the Windows 11 acrylic backdrop, so Blurman itself is frosted glass.
/// False on Windows versions without system backdrops, where the window stays opaque.
fn frost_window(hwnd: HWND) -> bool {
    let set_i32 = |attribute: DWMWINDOWATTRIBUTE, value: i32| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast(),
            size_of::<i32>() as u32,
        )
    };
    let set_color = |attribute: DWMWINDOWATTRIBUTE, value: u32| unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        )
    };
    let _ = set_i32(DWMWA_USE_IMMERSIVE_DARK_MODE, 1);
    // Same caption treatment as the pump screen window: round corners, plum border, light text.
    let _ = set_i32(DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND.0);
    let _ = set_color(DWMWA_BORDER_COLOR, 0x0030_2832);
    let _ = set_color(DWMWA_CAPTION_COLOR, 0x0018_1016);
    let _ = set_color(DWMWA_TEXT_COLOR, 0x00F2_EEF0);
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let frosted = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) }.is_ok()
        && set_i32(DWMWA_SYSTEMBACKDROP_TYPE, DWMSBT_TRANSIENTWINDOW.0).is_ok();
    if frosted {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(stay_frosted), 1, 0);
        }
    }
    frosted
}

/// Windows swaps the backdrop for a flat color while the window is inactive, judging by whether
/// the frame is drawn active. Always drawing it active keeps the glass. Keyboard focus is
/// tracked through other messages, so the window still knows when it is not focused.
unsafe extern "system" fn stay_frosted(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    if msg == WM_NCACTIVATE {
        return DefSubclassProc(hwnd, msg, WPARAM(1), lparam);
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Apps,
    Presets,
    Shortcuts,
    AutoPause,
    Settings,
}

impl Page {
    const ALL: [Page; 5] = [
        Page::Apps,
        Page::Presets,
        Page::Shortcuts,
        Page::AutoPause,
        Page::Settings,
    ];

    fn label(self) -> &'static str {
        match self {
            Page::Apps => "Apps",
            Page::Presets => "Presets",
            Page::Shortcuts => "Shortcuts",
            Page::AutoPause => "Auto-pause",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> NavIcon {
        match self {
            Page::Apps => NavIcon::Apps,
            Page::Presets => NavIcon::Presets,
            Page::Shortcuts => NavIcon::Shortcuts,
            Page::AutoPause => NavIcon::AutoPause,
            Page::Settings => NavIcon::Settings,
        }
    }
}

/// The three parts of the app editor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EditorTab {
    Look,
    Finish,
    Behavior,
}

/// Something that cannot be undone, waiting for a yes.
#[derive(Clone, PartialEq, Eq, Debug)]
enum Confirm {
    RestoreAll,
    RemoveRule(String),
    DeletePreset(String),
}

struct BlurmanApp {
    shared: Arc<Shared>,
    store: Store,
    settings: Settings,
    autostart: bool,
    tray_error: Option<String>,
    seen_generation: u64,
    groups: Vec<AppGroup>,
    scanned: Instant,
    page: Page,
    selected: Option<String>,
    /// What the editor shows: the selected app's rule, or what "Frost it" will save.
    look: Look,
    focus: FocusLook,
    only_titles: String,
    skip_titles: String,
    editor_tab: EditorTab,
    search: String,
    /// The shortcut waiting for its keys to be pressed.
    recording: Option<HotkeyAction>,
    /// The name being typed for a new preset, while that field is open.
    naming_preset: Option<String>,
    /// A preset being renamed: its old name and the new one so far.
    renaming: Option<(String, String)>,
    confirm: Option<Confirm>,
    toast: Option<(String, Instant)>,
    icons: IconCache,
    /// Launched by Windows startup: this session lives in the tray whatever the setting says.
    startup: bool,
    /// The window has a frosted backdrop showing through wherever nothing is drawn.
    glass: bool,
    /// A window saved from an older, smaller layout is widened once.
    window_fitted: bool,
    /// The 150% display was painting into a window sized as if it were 100%.
    dpi_matched: bool,
    /// When a slider edit should be written. None while the file matches the sliders.
    pending_save: Option<Instant>,
}

impl BlurmanApp {
    fn new(shared: Arc<Shared>, settings: Settings, startup: bool, glass: bool) -> Self {
        let store = rules::load();
        let tray_error = tray::sync(settings.close_to_tray || startup, &store).err();
        Self {
            shared,
            store,
            settings,
            autostart: autostart::is_enabled(),
            tray_error,
            seen_generation: 0,
            groups: target::list_groups(),
            scanned: Instant::now(),
            page: Page::Apps,
            selected: None,
            look: Look::default(),
            focus: FocusLook::Same,
            only_titles: String::new(),
            skip_titles: String::new(),
            editor_tab: EditorTab::Look,
            search: String::new(),
            recording: None,
            naming_preset: None,
            renaming: None,
            confirm: None,
            toast: None,
            icons: IconCache::default(),
            startup,
            glass,
            window_fitted: false,
            dpi_matched: false,
            pending_save: None,
        }
    }

    /// Debug builds only: open on a given page, app, tab, or dialog, so each screen can be
    /// checked without clicking through the window.
    #[cfg(debug_assertions)]
    fn preview_from_env(&mut self) {
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        if let Some(process) = var("BLURMAN_UI_SELECT") {
            self.select(process);
        }
        if let Some(page) = var("BLURMAN_UI_PAGE") {
            if let Some(found) = Page::ALL
                .into_iter()
                .find(|known| known.label().eq_ignore_ascii_case(&page))
            {
                self.page = found;
            }
        }
        match var("BLURMAN_UI_TAB").as_deref() {
            Some("finish") => self.editor_tab = EditorTab::Finish,
            Some("behavior") => self.editor_tab = EditorTab::Behavior,
            _ => {}
        }
        if var("BLURMAN_UI_CONFIRM").is_some() {
            self.confirm = Some(Confirm::RestoreAll);
        }
        if var("BLURMAN_UI_REDACT").is_some() {
            widgets::set_redact(true);
        }
        if let Some(text) = var("BLURMAN_UI_TOAST") {
            // Long enough to be captured.
            self.toast = Some((text, Instant::now() + Duration::from_secs(60)));
        }
    }

    fn wants_tray(&self) -> bool {
        self.settings.close_to_tray || self.startup
    }

    fn keeps_in_tray(&self) -> bool {
        self.wants_tray() && self.tray_error.is_none()
    }

    fn rescan(&mut self) {
        self.groups = target::list_groups();
        self.scanned = Instant::now();
    }

    /// Show a short confirmation near the bottom of the window.
    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    /// Pick up rule changes made from the tray, a shortcut, or the command line.
    fn sync_from_disk(&mut self) {
        let generation = self.shared.rules_generation.load(Ordering::SeqCst);
        if generation == self.seen_generation {
            return;
        }
        self.seen_generation = generation;
        let mut store = rules::load();
        // A drag that has not been written yet is newer than the file. Keep it, and hand the
        // merged rules to the effect thread, which just reloaded the older values.
        let unsaved = self.pending_save.is_some();
        if unsaved {
            keep_unsaved_looks(&mut store, &self.store);
        }
        self.store = store;
        self.sliders_from_rule();
        if unsaved {
            self.publish();
        }
    }

    fn sliders_from_rule(&mut self) {
        if let Some(rule) = self.selected_rule().cloned() {
            self.look = rule.look;
            self.focus = rule.focus;
            self.only_titles = rules::join_titles(&rule.only_titles);
            self.skip_titles = rules::join_titles(&rule.skip_titles);
        }
    }

    /// Open `process` in the editor. An app with no rule starts from the defaults, not from
    /// whatever the last app had.
    fn select(&mut self, process: String) {
        self.page = Page::Apps;
        self.selected = Some(process);
        self.naming_preset = None;
        if self.selected_rule().is_some() {
            self.sliders_from_rule();
        } else {
            self.look = Look::default();
            self.focus = FocusLook::Same;
            self.only_titles.clear();
            self.skip_titles.clear();
        }
    }

    fn selected_rule(&self) -> Option<&Rule> {
        self.selected
            .as_deref()
            .and_then(|process| self.store.rule(process))
    }

    /// Save the rules and have the effect thread reload them.
    fn publish(&mut self) {
        self.pending_save = None;
        self.store.normalize();
        if let Err(err) = rules::save(&self.store) {
            self.shared
                .set_status(format!("Could not save rules: {err}"));
            return;
        }
        ipc::signal(ipc::msg_reload());
    }

    /// Push a slider change to the panes now. The file is written once the drag settles.
    fn live_rule(&mut self, process: &str) {
        let Some(rule) = self.store.rule(process) else {
            return;
        };
        self.shared.push_tweak(Tweak {
            process: rule.process.clone(),
            look: rule.look,
        });
        ipc::signal(ipc::msg_tweak());
        self.pending_save = Some(Instant::now() + SAVE_AFTER);
    }

    fn flush_rules_file(&mut self) {
        self.pending_save = None;
        self.store.normalize();
        if let Err(err) = rules::save(&self.store) {
            self.shared
                .set_status(format!("Could not save rules: {err}"));
        }
    }

    /// Write the rules file once a drag ends, or about 80 ms after the last movement.
    fn settle_save(&mut self, pointer_down: bool) -> Duration {
        let Some(deadline) = self.pending_save else {
            return RESCAN;
        };
        if pointer_down && deadline > Instant::now() {
            return deadline
                .saturating_duration_since(Instant::now())
                .min(RESCAN);
        }
        self.flush_rules_file();
        RESCAN
    }

    /// The editor's look changed. Frosted windows follow along as you drag; a rule that is off
    /// just keeps the new look for when it is switched on.
    fn look_changed(&mut self) {
        let Some(process) = self.selected.clone() else {
            return;
        };
        let look = self.look.clamped();
        let paused = self.store.paused;
        let Some(rule) = self.store.rule_mut(&process) else {
            return;
        };
        rule.look = look;
        if rule.enabled && !paused {
            self.live_rule(&process);
        } else {
            self.pending_save = Some(Instant::now() + SAVE_AFTER);
        }
    }

    fn frost_selected(&mut self) {
        let Some(process) = self.selected.clone() else {
            return;
        };
        self.store.paused = false;
        let mut rule = rules::new_rule(&process, self.look);
        rule.focus = self.focus;
        rule.only_titles = rules::split_titles(&self.only_titles);
        rule.skip_titles = rules::split_titles(&self.skip_titles);
        self.store.upsert(rule);
        self.publish();
        self.notify(format!("Frosted {process}"));
    }

    fn set_enabled(&mut self, process: &str, on: bool) {
        if let Some(rule) = self.store.rule_mut(process) {
            rule.enabled = on;
            let name = rule.process.clone();
            self.publish();
            self.notify(if on {
                format!("Frosted {name}")
            } else {
                format!("{name} is back to normal")
            });
        }
    }

    /// Write behavior edits (focus and title filters) onto the selected rule, if it has one.
    fn behavior_to_rule(&mut self) {
        let Some(process) = self.selected.clone() else {
            return;
        };
        let (focus, only, skip) = (
            self.focus,
            rules::split_titles(&self.only_titles),
            rules::split_titles(&self.skip_titles),
        );
        if let Some(rule) = self.store.rule_mut(&process) {
            rule.focus = focus;
            rule.only_titles = only;
            rule.skip_titles = skip;
            self.publish();
        }
    }

    fn save_settings(&mut self) {
        match rules::save_settings(&self.settings) {
            Ok(()) => {
                ipc::signal(ipc::msg_settings());
            }
            Err(err) => self
                .shared
                .set_status(format!("Could not save settings: {err}")),
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.store.paused = paused;
        self.publish();
        self.notify(if paused {
            "Paused. Every app is back to normal."
        } else {
            "Frosting again"
        });
    }

    /// Carry out a confirmed action.
    fn confirmed(&mut self, confirm: Confirm) {
        match confirm {
            Confirm::RestoreAll => {
                self.store.clear();
                self.publish();
                self.notify("Every rule is deleted and every app is back to normal");
            }
            Confirm::RemoveRule(process) => {
                self.store.remove(&process);
                self.publish();
                self.notify(format!("Removed the rule for {process}"));
            }
            Confirm::DeletePreset(name) => {
                self.settings
                    .presets
                    .retain(|preset| !preset.name.eq_ignore_ascii_case(&name));
                self.save_settings();
                self.notify(format!("Deleted the {name} preset"));
            }
        }
    }

    /// Grow the window when Windows kept the physical size equal to the point size.
    ///
    /// On a 150% display egui then paints 1.5 pixels per point into that smaller
    /// window, and the right side is clipped.
    fn match_dpi(&mut self, ctx: &egui::Context) {
        if self.dpi_matched {
            return;
        }
        // Scale can show up a frame late. A 100% display never latches, and the check is tiny.
        let Some(ppp) = ctx.native_pixels_per_point() else {
            return;
        };
        if ppp < 1.2 {
            return;
        }
        let points = ctx.screen_rect().size();
        if points.x < 400.0 || points.y < 300.0 {
            return;
        }
        let hwnd = target::hwnd_of(self.shared.main_window.load(Ordering::SeqCst));
        if hwnd.0.is_null() {
            return;
        }
        if grow_window_to_pixels(hwnd, points, ppp) {
            self.dpi_matched = true;
        }
    }

    fn fit_window(&mut self, ctx: &egui::Context) {
        if self.window_fitted {
            return;
        }
        self.window_fitted = true;
        let small = ctx.input(|input| {
            input
                .viewport()
                .inner_rect
                .map(|rect| rect.width() < 880.0 || rect.height() < 580.0)
                .unwrap_or(true)
        });
        if small {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(WINDOW_SIZE.into()));
        }
    }

    /// The brand, what Blurman is doing right now with a pause control, and the pages.
    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 26.0), Sense::hover());
            ui.painter().circle_filled(rect.center(), 5.0, ACCENT);
            ui.label(RichText::new("Blurman").size(20.0).strong().color(TEXT));
        });
        ui.add_space(12.0);
        self.status_card(ui);
        ui.add_space(14.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        for page in Page::ALL {
            if widgets::nav_item(ui, page.icon(), page.label(), self.page == page).clicked() {
                self.page = page;
                self.recording = None;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::LEFT), |ui| {
            ui.add_space(4.0);
            ui.label(theme::muted(format!("Version {}", env!("CARGO_PKG_VERSION"))).small());
            let note = if self.keeps_in_tray() {
                "Closing keeps Blurman in the tray."
            } else {
                "Closing puts every app back."
            };
            ui.add(egui::Label::new(theme::muted(note).small()).wrap());
        });
    }

    fn status_card(&mut self, ui: &mut Ui) {
        let auto = self.shared.auto_paused.get();
        let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
        let (title, color) = if self.store.paused {
            ("Paused".to_string(), WARN)
        } else if auto.is_some() {
            ("Auto-paused".to_string(), WARN)
        } else if frosted == 0 {
            ("Nothing frosted yet".to_string(), MUTED)
        } else if frosted == 1 {
            ("Frosting 1 app".to_string(), OK)
        } else {
            (format!("Frosting {frosted} apps"), OK)
        };
        egui::Frame::new()
            .fill(color.gamma_multiply(0.12))
            .stroke(egui::Stroke::new(1.0_f32, color.gamma_multiply(0.4)))
            .corner_radius(12)
            .inner_margin(egui::Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(10.0, 18.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 4.0, color);
                    ui.label(RichText::new(title).color(TEXT));
                });
                if let Some(reason) = auto.filter(|_| !self.store.paused) {
                    ui.add(egui::Label::new(theme::muted(reason).small()).wrap());
                }
                ui.add_space(4.0);
                let any = !self.store.rules.is_empty();
                if self.store.paused {
                    if ui
                        .add(
                            theme::primary_button("Resume")
                                .min_size(egui::vec2(ui.available_width(), 30.0)),
                        )
                        .clicked()
                    {
                        self.set_paused(false);
                    }
                } else if ui
                    .add_enabled(
                        any,
                        egui::Button::new("Pause all")
                            .min_size(egui::vec2(ui.available_width(), 30.0)),
                    )
                    .on_hover_text("Put every app back without deleting rules.")
                    .clicked()
                {
                    self.set_paused(true);
                }
            });
    }

    /// Notes that apply to every page: an update, a problem, or a missing feature.
    fn banners(&mut self, ui: &mut Ui) {
        if let Some(update) = self.shared.update.get() {
            widgets::banner(ui, ACCENT, |ui| {
                ui.label(RichText::new(format!("Blurman {} is out.", update.version)).color(TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Not now").clicked() {
                        self.shared.set_update(None);
                    }
                    if ui
                        .add(theme::primary_button("Download").min_size(egui::vec2(100.0, 26.0)))
                        .clicked()
                    {
                        system::open(&update.url);
                    }
                });
            });
        }
        let status = self.shared.status();
        if !status.is_empty() {
            widgets::banner(ui, WARN, |ui| {
                ui.add(egui::Label::new(RichText::new(status).color(WARN)).wrap());
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.small_button("Dismiss").clicked() {
                        self.shared.set_status("");
                    }
                });
            });
        }
        if self.shared.fallback.load(Ordering::SeqCst) {
            widgets::banner(ui, MUTED, |ui| {
                ui.add(
                    egui::Label::new(theme::muted(
                        "This PC uses system acrylic, so the blur slider sets how milky the glass is.",
                    ))
                    .wrap(),
                );
            });
        }
    }

    fn confirm_dialog(&mut self, ctx: &egui::Context) {
        let Some(confirm) = self.confirm.clone() else {
            return;
        };
        let (title, body, action) = match &confirm {
            Confirm::RestoreAll => (
                "Delete every rule?".to_string(),
                "Every app goes back to normal and every saved rule is deleted. Pause all keeps the rules instead.".to_string(),
                "Delete all rules",
            ),
            Confirm::RemoveRule(process) => (
                format!("Remove the rule for {process}?"),
                "The app goes back to normal and its look and behavior are forgotten. Switching it off keeps them instead.".to_string(),
                "Remove rule",
            ),
            Confirm::DeletePreset(name) => (
                format!("Delete the {name} preset?"),
                "Apps using this look keep it. Only the preset goes.".to_string(),
                "Delete preset",
            ),
        };
        let mut answer = None;
        let modal = egui::Modal::new(egui::Id::new("confirm"))
            .frame(theme::dialog())
            .show(ctx, |ui| {
                ui.set_width(380.0);
                ui.label(RichText::new(title).size(17.0).strong().color(TEXT));
                ui.add_space(4.0);
                ui.add(egui::Label::new(theme::muted(body)).wrap());
                ui.add_space(14.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.add(widgets::danger_button(action)).clicked() {
                        answer = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        answer = Some(false);
                    }
                });
            });
        if modal.should_close() && answer.is_none() {
            answer = Some(false);
        }
        match answer {
            Some(true) => {
                self.confirm = None;
                self.confirmed(confirm);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    /// Draw the confirmation message, and say how soon to repaint for its fade.
    fn draw_toast(&mut self, ctx: &egui::Context) -> Option<Duration> {
        if let Some(notice) = self.shared.notice.take() {
            self.notify(notice);
        }
        let (text, since) = self.toast.as_ref()?;
        let age = since.elapsed();
        if age >= TOAST_FOR {
            self.toast = None;
            return None;
        }
        let left = TOAST_FOR - age;
        let opacity = (left.as_secs_f32() / 0.4).min(1.0);
        widgets::toast(ctx, text, opacity);
        Some(if left < Duration::from_millis(400) {
            Duration::from_millis(16)
        } else {
            left - Duration::from_millis(400)
        })
    }
}

impl eframe::App for BlurmanApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.glass {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            theme::BG.to_normalized_gamma_f32()
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.viewport().close_requested()) {
            // A drag can end by closing the window, before the settle timer writes the file.
            if self.pending_save.is_some() {
                self.flush_rules_file();
            }
            if self.keeps_in_tray() && !self.shared.exiting.load(Ordering::SeqCst) {
                self.shared.to_tray.store(true, Ordering::SeqCst);
            }
        }
        self.sync_from_disk();
        if let Some(process) = self.shared.picked.take() {
            self.rescan();
            self.select(process);
        }
        self.record_shortcut(ctx);
        if self.scanned.elapsed() >= RESCAN {
            self.rescan();
        }
        if self.wants_tray() {
            self.tray_error = tray::sync(true, &self.store).err();
        }
        self.fit_window(ctx);
        self.match_dpi(ctx);

        egui::SidePanel::left("nav")
            .resizable(false)
            .exact_width(SIDEBAR_WIDTH)
            .frame(theme::sidebar(self.glass))
            .show(ctx, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(theme::page(self.glass))
            .show(ctx, |ui| {
                self.banners(ui);
                match self.page {
                    Page::Apps => self.apps_page(ui),
                    Page::Presets => self.presets_page(ui),
                    Page::Shortcuts => self.shortcuts_page(ui),
                    Page::AutoPause => self.auto_pause_page(ui),
                    Page::Settings => self.settings_page(ui),
                }
            });
        self.confirm_dialog(ctx);
        let toast = self.draw_toast(ctx);
        let pointer_down = ctx.input(|input| input.pointer.any_down());
        let mut next = self.settle_save(pointer_down);
        if let Some(toast) = toast {
            next = next.min(toast);
        }
        ctx.request_repaint_after(next);
    }
}

/// Copy slider values from `local` onto the matching rules in `disk`. Adding, removing, pausing,
/// and switching rules on or off are written at once, so the file is right about those.
fn keep_unsaved_looks(disk: &mut Store, local: &Store) {
    for rule in &mut disk.rules {
        if let Some(mine) = local.rule(&rule.process) {
            rule.look = mine.look;
        }
    }
}

/// If the client area is still the point size, size it to `points * pixels_per_point`.
/// Returns true once the window is the right size or this pass changed it.
fn grow_window_to_pixels(hwnd: HWND, points: egui::Vec2, pixels_per_point: f32) -> bool {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, SetWindowPos, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER,
    };

    let wanted_w = (points.x * pixels_per_point).round() as i32;
    let wanted_h = (points.y * pixels_per_point).round() as i32;
    unsafe {
        let mut client = RECT::default();
        if GetClientRect(hwnd, &mut client).is_err() {
            return false;
        }
        let client_w = client.right - client.left;
        let client_h = client.bottom - client.top;
        if (client_w - wanted_w).abs() <= 12 && (client_h - wanted_h).abs() <= 12 {
            return true;
        }
        // The broken case is a client area that matches the point size, not the pixel size.
        let width_ratio = client_w as f32 / points.x;
        let height_ratio = client_h as f32 / points.y;
        let sized_in_points =
            (0.85..1.15).contains(&width_ratio) && (0.85..1.15).contains(&height_ratio);
        if !sized_in_points {
            return false;
        }
        let mut window = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() {
            return false;
        }
        let chrome_w = (window.right - window.left) - client_w;
        let chrome_h = (window.bottom - window.top) - client_h;
        SetWindowPos(
            hwnd,
            None,
            0,
            0,
            wanted_w + chrome_w,
            wanted_h + chrome_h,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .is_ok()
    }
}

fn decode_icon(bytes: &[u8]) -> egui::IconData {
    let image = image::load_from_memory(bytes).expect("app icon");
    let rgba = image.to_rgba8();
    egui::IconData {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
    }
}

pub fn window_icon() -> egui::IconData {
    decode_icon(include_bytes!("../../assets/icon-256.png"))
}

pub fn tray_icon() -> egui::IconData {
    decode_icon(include_bytes!("../../assets/icon-32.png"))
}

#[cfg(test)]
mod tests;
