//! The Blurman window.

use crate::autostart;
use crate::hotkey;
use crate::ipc;
use crate::mapping::{self, BlurStyle, FocusLook, Look, Rgb, TintSource};
use crate::picker;
use crate::rules::{self, HotkeyAction, Preset, Rule, Settings, Store};
use crate::shared::{Shared, Tweak};
use crate::system;
use crate::target::{self, AppGroup};
use crate::theme::{self, ACCENT, MUTED, OK, ROW_HOVER, ROW_SELECTED, TEXT, WARN};
use crate::tray;
use egui::{
    Align, Color32, CursorIcon, Frame, Layout, Margin, RichText, Sense, Shape, Slider, Stroke, Ui,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
const PRESET_NAME: &str = "preset-name";

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
            .with_inner_size([1000.0, 680.0])
            .with_min_inner_size([900.0, 560.0])
            .with_transparent(true)
            .with_icon(window_icon()),
        ..Default::default()
    };
    let app_shared = shared.clone();
    let result = eframe::run_native(
        "Blurman",
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
            Ok(Box::new(BlurmanApp::new(
                app_shared,
                settings,
                tray_session,
                glass,
                &cc.egui_ctx,
            )))
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

struct BlurmanApp {
    shared: Arc<Shared>,
    store: Store,
    settings: Settings,
    autostart: bool,
    tray_error: Option<String>,
    seen_generation: u64,
    groups: Vec<AppGroup>,
    scanned: Instant,
    selected: Option<String>,
    /// What the Glass card shows: the selected app's rule, or what "Frost it" will save.
    look: Look,
    focus: FocusLook,
    only_titles: String,
    skip_titles: String,
    /// The shortcut waiting for its keys to be pressed.
    recording: Option<HotkeyAction>,
    /// The name being typed for a new preset, while that field is open.
    naming_preset: Option<String>,
    /// Launched by Windows startup: this session lives in the tray whatever the setting says.
    startup: bool,
    /// The window has a frosted backdrop showing through wherever nothing is drawn.
    glass: bool,
    /// A window saved from the old narrow layout is widened once.
    window_fitted: bool,
    /// The 150% display was painting into a window sized as if it were 100%.
    dpi_matched: bool,
    /// When a slider edit should be written. None while the file matches the sliders.
    pending_save: Option<Instant>,
}

impl BlurmanApp {
    fn new(
        shared: Arc<Shared>,
        settings: Settings,
        startup: bool,
        glass: bool,
        _ctx: &egui::Context,
    ) -> Self {
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
            selected: None,
            look: Look::default(),
            focus: FocusLook::Same,
            only_titles: String::new(),
            skip_titles: String::new(),
            recording: None,
            naming_preset: None,
            startup,
            glass,
            window_fitted: false,
            dpi_matched: false,
            pending_save: None,
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

    /// Pick up rule changes made from the tray or the command line.
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

    /// A newly picked app with no rule starts from a clean slate, not the last app's filters.
    fn select(&mut self, process: String) {
        self.selected = Some(process);
        if self.selected_rule().is_some() {
            self.sliders_from_rule();
        } else {
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

    fn unfrost_selected(&mut self) {
        if let Some(process) = self.selected.clone() {
            self.store.remove(&process);
            self.publish();
        }
    }

    fn set_paused(&mut self, paused: bool) {
        self.store.paused = paused;
        self.publish();
    }

    /// Grow the window when Windows kept the physical size equal to the point size.
    ///
    /// On a 150% display egui then paints 1.5 pixels per point into that smaller
    /// window, and the right column is clipped.
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
        let narrow = ctx.input(|input| {
            input
                .viewport()
                .inner_rect
                .map(|rect| rect.width() < 900.0)
                .unwrap_or(true)
        });
        if narrow {
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(1000.0, 680.0)));
        }
    }

    fn header(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let auto_paused = self.shared.auto_paused.get().is_some();
            let (rect, _) = ui.allocate_exact_size(egui::vec2(14.0, 22.0), Sense::hover());
            let mark = if self.store.paused || auto_paused {
                WARN
            } else {
                ACCENT
            };
            ui.painter().circle_filled(rect.center(), 5.0, mark);
            ui.heading(RichText::new("Blurman").color(TEXT));
            ui.label(theme::muted("Frosted glass behind the apps you pick"));
            let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
            let (text, color) = if self.store.paused {
                ("Paused".to_string(), WARN)
            } else if auto_paused {
                ("Auto-paused".to_string(), WARN)
            } else if frosted == 0 {
                ("Ready".to_string(), MUTED)
            } else if frosted == 1 {
                ("1 frosted".to_string(), OK)
            } else {
                (format!("{frosted} frosted"), OK)
            };
            theme::status_pill(ui, &text, color);
        });
    }

    fn banners(&mut self, ui: &mut Ui) {
        if let Some(update) = self.shared.update.get() {
            banner(ui, ACCENT, |ui| {
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
        if let Some(reason) = self.shared.auto_paused.get().filter(|_| !self.store.paused) {
            banner(ui, WARN, |ui| {
                ui.add(
                    egui::Label::new(
                        RichText::new(format!(
                            "Auto-paused: {reason}. Apps frost again when that ends."
                        ))
                        .color(WARN),
                    )
                    .wrap(),
                );
            });
        }
        if self.store.paused {
            banner(ui, WARN, |ui| {
                ui.label(RichText::new("Paused. Every app is back to normal.").color(WARN));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add(theme::primary_button("Resume").min_size(egui::vec2(80.0, 26.0)))
                        .clicked()
                    {
                        self.set_paused(false);
                    }
                });
            });
        }
        let status = self.shared.status();
        if !status.is_empty() {
            banner(ui, WARN, |ui| {
                ui.add(egui::Label::new(RichText::new(status).color(WARN)).wrap());
            });
        }
        if self.shared.fallback.load(Ordering::SeqCst) {
            banner(ui, MUTED, |ui| {
                ui.add(
                    egui::Label::new(theme::muted(
                        "This PC uses system acrylic, so the blur slider sets how milky the glass is.",
                    ))
                    .wrap(),
                );
            });
        }
    }

    /// Two equal columns that stay inside the window.
    ///
    /// `Ui::columns_const` widens both sides to whichever is wider. A vertical
    /// scroll area then grows to that width and the window clips the right side.
    /// The column rect is tall on purpose: inside a scroll area the available
    /// height is only the viewport, and a column that tall clips the lower cards
    /// so they never become something you can scroll to.
    fn two_columns(&mut self, ui: &mut Ui) {
        let gap = ui.spacing().item_spacing.x;
        let total = ui.available_width();
        let col = ((total - gap) / 2.0).max(0.0);
        let top = ui.cursor().min;
        let bottom = top.y + 100_000.0;
        let mut left = column_ui(
            ui,
            egui::Rect::from_min_max(top, egui::pos2(top.x + col, bottom)),
        );
        let mut right = column_ui(
            ui,
            egui::Rect::from_min_max(
                top + egui::vec2(col + gap, 0.0),
                egui::pos2(top.x + total, bottom),
            ),
        );
        self.running_apps(&mut left);
        left.add_space(8.0);
        self.shortcuts_card(&mut left);
        left.add_space(8.0);
        self.auto_pause_card(&mut left);
        self.glass_controls(&mut right);
        right.add_space(8.0);
        self.startup_card(&mut right);
        right.add_space(8.0);
        self.files_card(&mut right);
        let used = left.min_rect().height().max(right.min_rect().height());
        ui.allocate_rect(
            egui::Rect::from_min_size(top, egui::vec2(total, used)),
            Sense::hover(),
        );
    }

    fn running_apps(&mut self, ui: &mut Ui) {
        let mut picked = None;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::card_title("Running apps"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Refresh").clicked() {
                        self.rescan();
                    }
                    if ui
                        .button("Pick a window")
                        .on_hover_text(
                            "Blurman steps aside. Click any window to pick its app, or press Esc.",
                        )
                        .clicked()
                    {
                        ui.ctx()
                            .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        picker::start(self.shared.clone());
                    }
                });
            });
            ui.add_space(2.0);
            egui::ScrollArea::vertical()
                .id_salt("running")
                .max_height(250.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    if self.groups.is_empty() {
                        ui.label(theme::muted("No normal windows are open."));
                    }
                    ui.spacing_mut().item_spacing.y = 2.0;
                    for group in &self.groups {
                        let selected = self.selected.as_deref() == Some(group.process.as_str());
                        let rule = self.store.rule(&group.process).filter(|rule| rule.enabled);
                        if app_row(ui, group, selected, rule).clicked() {
                            picked = Some(group.process.clone());
                        }
                    }
                });
        });
        if let Some(process) = picked {
            self.select(process);
        }
    }

    fn glass_controls(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            let ruled = self.selected_rule().is_some();
            ui.horizontal(|ui| {
                let full = ui.available_width();
                ui.set_max_width(full);
                ui.label(theme::card_title("Glass"));
                if let Some(process) = &self.selected {
                    ui.label(theme::muted("for"));
                    ui.add(
                        egui::Label::new(RichText::new(process).strong().color(ACCENT)).truncate(),
                    );
                }
            });
            if self.selected.is_none() {
                ui.add(
                    egui::Label::new(theme::muted(
                        "Pick an app on the left, set the look, then frost it.",
                    ))
                    .wrap(),
                );
            }
            ui.add_space(4.0);
            let fallback = self.shared.fallback.load(Ordering::SeqCst);
            let mut moved = false;
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::muted("Look"));
                moved |= look_picker(ui, &mut self.look.style);
            });
            let milky = self.look.style == BlurStyle::Acrylic || fallback;
            moved |= labeled_slider(ui, "Transparency", |ui| {
                ui.add(
                    Slider::new(
                        &mut self.look.transparency,
                        mapping::TRANSPARENCY_MIN..=mapping::TRANSPARENCY_MAX,
                    )
                    .suffix("%"),
                )
            });
            moved |= labeled_slider(ui, if milky { "Milkiness" } else { "Blur" }, |ui| {
                ui.add(Slider::new(
                    &mut self.look.blur,
                    mapping::BLUR_MIN..=mapping::BLUR_MAX,
                ))
            });
            if !fallback {
                ui.label(
                    theme::muted(match self.look.style {
                        BlurStyle::Frost => "Blurs whatever is behind the window.",
                        BlurStyle::Acrylic => {
                            "Milky system glass. The slider sets how milky it is."
                        }
                    })
                    .small(),
                );
            }
            moved |= self.presets_row(ui);
            moved |= finish_controls(ui, &mut self.look, milky);
            if self.behavior_controls(ui) && ruled {
                self.behavior_to_rule();
            }
            if ruled && moved {
                let was_enabled = self.selected_rule().is_some_and(|rule| rule.enabled);
                if self.store.paused || !was_enabled {
                    // Enabling, or waking from pause, has to attach panes. A pure slider edit does not.
                    self.frost_selected();
                } else if let Some(process) = self.selected.clone() {
                    let look = self.look;
                    if let Some(rule) = self.store.rule_mut(&process) {
                        rule.look = look.clamped();
                    }
                    self.live_rule(&process);
                }
            }
            ui.add_space(6.0);
            ui.add_enabled_ui(self.selected.is_some(), |ui| {
                ui.horizontal_wrapped(|ui| {
                    if ruled {
                        if ui.button("Remove frost").clicked() {
                            self.unfrost_selected();
                        }
                        ui.label(theme::muted("Changes apply as you drag."));
                    } else if ui.add(theme::primary_button("Frost it")).clicked() {
                        self.frost_selected();
                    }
                });
            });
        });
    }

    /// One click puts a saved look on the sliders. Right-click a preset to delete it.
    fn presets_row(&mut self, ui: &mut Ui) -> bool {
        let mut picked = None;
        let mut delete = None;
        let mut save = false;
        let mut cancel = false;
        ui.add_space(2.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(theme::muted("Presets"));
            for preset in &self.settings.presets {
                let response = theme::choice(ui, preset.look == self.look, &preset.name)
                    .on_hover_text("Use this look. Right-click to delete it.");
                if response.clicked() {
                    picked = Some(preset.look);
                }
                response.context_menu(|ui| {
                    if ui.button("Delete preset").clicked() {
                        delete = Some(preset.name.clone());
                        ui.close_menu();
                    }
                });
            }
            match &mut self.naming_preset {
                None => {
                    if ui
                        .button("Save as…")
                        .on_hover_text("Keep this look as a preset.")
                        .clicked()
                    {
                        self.naming_preset = Some(String::new());
                        ui.memory_mut(|memory| memory.request_focus(egui::Id::new(PRESET_NAME)));
                    }
                }
                Some(name) => {
                    let field = ui.add(
                        egui::TextEdit::singleline(name)
                            .id(egui::Id::new(PRESET_NAME))
                            .hint_text("Preset name")
                            .desired_width(110.0),
                    );
                    let entered =
                        field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Save").clicked() || entered {
                        save = true;
                    }
                    if ui.button("Cancel").clicked()
                        || ui.input(|i| i.key_pressed(egui::Key::Escape))
                    {
                        cancel = true;
                    }
                }
            }
        });
        if cancel {
            self.naming_preset = None;
        }
        if save {
            let name = self.naming_preset.take().unwrap_or_default();
            let name = name.trim();
            if !name.is_empty() {
                self.settings.save_preset(Preset {
                    name: name.to_string(),
                    look: self.look.clamped(),
                });
                self.save_settings();
            }
        }
        if let Some(name) = delete {
            self.settings
                .presets
                .retain(|preset| !preset.name.eq_ignore_ascii_case(&name));
            self.save_settings();
        }
        match picked {
            Some(look) if look != self.look => {
                self.look = look;
                true
            }
            _ => false,
        }
    }

    /// Focus look and title filters. Returns whether any of them changed and should be saved.
    fn behavior_controls(&mut self, ui: &mut Ui) -> bool {
        let mut changed = false;
        egui::CollapsingHeader::new(RichText::new("Behavior").color(TEXT))
            .id_salt("behavior")
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::muted("While you use it"));
                    for (focus, label, tip) in [
                        (FocusLook::Same, "Same", "Stay frosted while focused."),
                        (
                            FocusLook::Lighter,
                            "Lighter",
                            "Half as see-through while focused.",
                        ),
                        (
                            FocusLook::Solid,
                            "Solid",
                            "Solid while focused, frosted again when you move on.",
                        ),
                    ] {
                        if theme::choice(ui, self.focus == focus, label)
                            .on_hover_text(tip)
                            .clicked()
                            && self.focus != focus
                        {
                            self.focus = focus;
                            changed = true;
                        }
                    }
                });
                changed |= title_field(
                    ui,
                    "Only titles",
                    &mut self.only_titles,
                    "Frost only windows with these in the title",
                );
                changed |= title_field(
                    ui,
                    "Skip titles",
                    &mut self.skip_titles,
                    "Leave windows with these in the title alone",
                );
                ui.label(
                    theme::muted("Separate titles with |. Use * as a wildcard, like *- YouTube*.")
                        .small(),
                );
            });
        changed
    }

    fn saved_rules(&mut self, ui: &mut Ui) {
        let mut structural = false;
        let mut tweaked = Vec::new();
        let mut delete = None;
        let mut edit = None;
        let mut pause = None;
        let mut restore_all = false;
        theme::card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(theme::card_title("Saved rules"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let any = !self.store.rules.is_empty();
                    if ui
                        .add_enabled(any, egui::Button::new("Restore all"))
                        .on_hover_text("Delete every rule and put every app back.")
                        .clicked()
                    {
                        restore_all = true;
                    }
                    let label = if self.store.paused { "Resume" } else { "Pause" };
                    if ui
                        .add_enabled(any, egui::Button::new(label))
                        .on_hover_text("Put every app back without deleting rules.")
                        .clicked()
                    {
                        pause = Some(!self.store.paused);
                    }
                });
            });
            if self.store.rules.is_empty() {
                ui.label(theme::muted("Nothing is frosted yet."));
                return;
            }
            ui.add_space(2.0);
            for rule in &mut self.store.rules {
                Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(255, 255, 255, 10))
                    .stroke(Stroke::new(
                        1.0_f32,
                        Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                    ))
                    .corner_radius(12)
                    .inner_margin(Margin::symmetric(12, 10))
                    .show(ui, |ui| {
                        let row_w = ui.available_width();
                        ui.set_max_width(row_w);
                        let heading = rule_heading(ui, &mut rule.enabled, &rule.process);
                        structural |= heading.toggled;
                        if heading.remove {
                            delete = Some(rule.process.clone());
                        }
                        if heading.edit {
                            edit = Some(rule.process.clone());
                        }
                        ui.horizontal_wrapped(|ui| {
                            ui.label(theme::muted("Look"));
                            let look = look_picker(ui, &mut rule.look.style);
                            if look && !tweaked.iter().any(|item: &String| item == &rule.process) {
                                tweaked.push(rule.process.clone());
                            }
                            if let Some(extra) = rule_extras(rule) {
                                ui.label(theme::muted(extra).small());
                            }
                        });
                        let milky = rule.look.style == BlurStyle::Acrylic;
                        if rule_slider_row(
                            ui,
                            &mut rule.look.transparency,
                            &mut rule.look.blur,
                            milky,
                        ) && !tweaked.iter().any(|item: &String| item == &rule.process)
                        {
                            tweaked.push(rule.process.clone());
                        }
                    });
            }
        });
        if let Some(process) = edit {
            self.select(process);
        }
        if let Some(process) = delete {
            self.store.remove(&process);
            structural = true;
        }
        if restore_all {
            self.store.clear();
            structural = true;
        }
        if let Some(paused) = pause {
            self.store.paused = paused;
            structural = true;
        }
        if structural {
            self.sliders_from_rule();
            self.publish();
        } else if !tweaked.is_empty() {
            self.sliders_from_rule();
            for process in tweaked {
                self.live_rule(&process);
            }
        }
    }

    fn startup_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.label(theme::card_title("Startup and tray"));
            ui.add_space(4.0);
            let mut autostart = self.autostart;
            if ui
                .checkbox(&mut autostart, "Start when I sign in to Windows")
                .changed()
            {
                match autostart::set_enabled(autostart) {
                    Ok(()) => self.autostart = autostart,
                    Err(err) => self.shared.set_status(err),
                }
            }
            if ui
                .checkbox(
                    &mut self.settings.close_to_tray,
                    "Keep running in the tray when the window closes",
                )
                .changed()
            {
                self.save_settings();
                self.tray_error = tray::sync(self.wants_tray(), &self.store).err();
            }
            if let Some(err) = &self.tray_error {
                ui.colored_label(WARN, err);
            }
            ui.label(theme::muted(
                "Click the tray icon to show this window. Right-click it to pause, switch apps on or off, or exit.",
            ));
            if ui
                .checkbox(
                    &mut self.settings.check_updates,
                    "Check for updates once a day",
                )
                .on_hover_text("Asks GitHub for the latest release. Nothing is downloaded.")
                .changed()
            {
                if !self.settings.check_updates {
                    self.shared.set_update(None);
                }
                self.save_settings();
            }
        });
    }

    fn shortcuts_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            let mut changed = false;
            ui.horizontal(|ui| {
                ui.label(theme::card_title("Shortcuts"));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    changed |= ui
                        .checkbox(&mut self.settings.hotkeys.enabled, "On")
                        .on_hover_text("Work from any app, even while Blurman is in the tray.")
                        .changed();
                });
            });
            ui.add_enabled_ui(self.settings.hotkeys.enabled, |ui| {
                for action in HotkeyAction::ALL {
                    let row_w = ui.available_width();
                    ui.horizontal(|ui| {
                        ui.set_width(row_w);
                        ui.label(theme::muted(action.label()));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let recording = self.recording == Some(action);
                            let current = self.settings.hotkeys.get(action);
                            let text = if recording {
                                "Press keys…".to_string()
                            } else if current.is_empty() {
                                "Off".to_string()
                            } else {
                                current.to_string()
                            };
                            let button = if recording {
                                egui::Button::new(RichText::new(text).color(Color32::WHITE))
                                    .fill(ACCENT)
                            } else {
                                egui::Button::new(RichText::new(text).monospace())
                            };
                            if ui
                                .add(button.min_size(egui::vec2(150.0, 0.0)))
                                .on_hover_text(
                                    "Click, then press the keys. Backspace turns it off, Esc keeps it.",
                                )
                                .clicked()
                            {
                                self.recording = if recording { None } else { Some(action) };
                            }
                        });
                    });
                }
            });
            let problem = self.shared.hotkey_problem.get();
            if !problem.is_empty() && self.settings.hotkeys.enabled {
                ui.add(egui::Label::new(RichText::new(problem).color(WARN)).wrap());
            }
            if changed {
                self.recording = None;
                self.save_settings();
            }
        });
    }

    /// While a shortcut is recording, the next key press with Ctrl or Alt becomes the shortcut.
    /// The keys are taken out of the input so they do not also press a button.
    fn record_shortcut(&mut self, ctx: &egui::Context) {
        let Some(action) = self.recording else {
            return;
        };
        let events = ctx.input_mut(|input| {
            let events = input.events.clone();
            input
                .events
                .retain(|event| !matches!(event, egui::Event::Key { .. } | egui::Event::Text(_)));
            events
        });
        for event in events {
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                continue;
            };
            let combo = match key {
                egui::Key::Escape => {
                    self.recording = None;
                    return;
                }
                egui::Key::Backspace | egui::Key::Delete if !modifiers.any() => String::new(),
                _ => match hotkey::from_egui(key, modifiers) {
                    Some(combo) => combo,
                    None => continue,
                },
            };
            let taken = HotkeyAction::ALL.into_iter().find(|other| {
                *other != action
                    && !combo.is_empty()
                    && self
                        .settings
                        .hotkeys
                        .get(*other)
                        .eq_ignore_ascii_case(&combo)
            });
            if let Some(other) = taken {
                self.shared.set_status(format!(
                    "{combo} is already used for \"{}\".",
                    other.label()
                ));
            } else {
                self.settings.hotkeys.set(action, combo);
                self.save_settings();
            }
            self.recording = None;
            return;
        }
    }

    fn auto_pause_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.label(theme::card_title("Auto-pause"));
            ui.add_space(2.0);
            let auto = &mut self.settings.auto_pause;
            let mut changed = ui
                .checkbox(&mut auto.battery_saver, "When battery saver is on")
                .changed();
            changed |= ui
                .checkbox(
                    &mut auto.quiet,
                    "During Do not disturb or presentation mode",
                )
                .changed();
            ui.horizontal(|ui| {
                changed |= ui.checkbox(&mut auto.schedule, "Between").changed();
                changed |= clock_field(ui, "auto-from", &mut auto.from);
                ui.label("and");
                changed |= clock_field(ui, "auto-until", &mut auto.until);
            });
            ui.add(
                egui::Label::new(theme::muted(
                    "Rules stay saved, and apps frost again when the condition ends.",
                ))
                .wrap(),
            );
            if changed {
                self.save_settings();
            }
        });
    }

    fn files_card(&mut self, ui: &mut Ui) {
        theme::card(ui, |ui| {
            ui.label(theme::card_title("Files"));
            ui.label(theme::muted("Rules and settings are saved in"));
            let dir = rules::app_dir();
            ui.add(
                egui::Label::new(
                    RichText::new(dir.display().to_string())
                        .monospace()
                        .color(TEXT),
                )
                .wrap(),
            );
            if ui.button("Open folder").clicked() {
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::process::Command::new("explorer").arg(&dir).spawn();
            }
            ui.label(
                theme::muted(format!(
                    "Blurman {}. The app is faded, and a blurred glass window sits right behind it.",
                    env!("CARGO_PKG_VERSION")
                ))
                .small(),
            );
        });
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
        let footer = if self.keeps_in_tray() {
            "Closing hides Blurman in the tray. Exit from the tray puts every app back."
        } else {
            "Closing Blurman puts every app back to normal."
        };
        egui::CentralPanel::default()
            .frame(theme::shell(self.glass))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.header(ui);
                        ui.add_space(8.0);
                        self.banners(ui);
                        self.two_columns(ui);
                        ui.add_space(8.0);
                        self.saved_rules(ui);
                        ui.add_space(8.0);
                        ui.label(RichText::new(footer).size(13.5).color(MUTED));
                    });
            });
        let pointer_down = ctx.input(|input| input.pointer.any_down());
        ctx.request_repaint_after(self.settle_save(pointer_down));
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

/// Tint, saturation, and grain. Acrylic only takes the tint color; the rest needs frost.
fn finish_controls(ui: &mut Ui, look: &mut Look, milky: bool) -> bool {
    let mut changed = false;
    egui::CollapsingHeader::new(RichText::new("Finish").color(TEXT))
        .id_salt("finish")
        .show(ui, |ui| {
            let finish = &mut look.finish;
            ui.horizontal_wrapped(|ui| {
                ui.label(theme::muted("Tint"));
                for (source, label, tip) in [
                    (TintSource::Custom, "Color", "A color you pick."),
                    (
                        TintSource::Theme,
                        "Theme",
                        "Dark or light, following the Windows app mode.",
                    ),
                    (
                        TintSource::Accent,
                        "Accent",
                        "The Windows accent color, following it as it changes.",
                    ),
                ] {
                    if theme::choice(ui, finish.tint_source == source, label)
                        .on_hover_text(tip)
                        .clicked()
                        && finish.tint_source != source
                    {
                        finish.tint_source = source;
                        changed = true;
                    }
                }
                if finish.tint_source == TintSource::Custom {
                    let mut rgb = [finish.tint.0, finish.tint.1, finish.tint.2];
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                        finish.tint = Rgb(rgb[0], rgb[1], rgb[2]);
                        changed = true;
                    }
                }
            });
            if milky {
                ui.add(
                    egui::Label::new(
                        theme::muted(
                            "Acrylic is colored with the tint, and Milkiness sets how much. Saturation and grain need Frost.",
                        )
                        .small(),
                    )
                    .wrap(),
                );
                return;
            }
            changed |= labeled_slider(ui, "Tint amount", |ui| {
                ui.add(Slider::new(&mut finish.tint_amount, 0..=mapping::TINT_MAX).suffix("%"))
            });
            changed |= labeled_slider(ui, "Saturation", |ui| {
                ui.add(
                    Slider::new(
                        &mut finish.saturation,
                        mapping::SATURATION_MIN..=mapping::SATURATION_MAX,
                    )
                    .suffix("%"),
                )
            });
            changed |= labeled_slider(ui, "Grain", |ui| {
                ui.add(Slider::new(&mut finish.grain, 0..=mapping::GRAIN_MAX))
            });
        });
    changed
}

/// A label and a text field for title patterns. Saved when you leave the field or press Enter.
fn title_field(ui: &mut Ui, label: &str, text: &mut String, hint: &str) -> bool {
    let mut done = false;
    let row_w = ui.available_width();
    ui.horizontal(|ui| {
        ui.set_width(row_w);
        // Both title rows share a label width, so their fields line up.
        let size = egui::TextStyle::Body.resolve(ui.style()).size;
        let label_w = text_width(ui, "Only titles", size).max(text_width(ui, "Skip titles", size));
        ui.allocate_ui_with_layout(
            egui::vec2(label_w + 6.0, ui.spacing().interact_size.y),
            Layout::left_to_right(Align::Center),
            |ui| ui.label(theme::muted(label)),
        );
        let field = ui.add(
            egui::TextEdit::singleline(text)
                .hint_text(hint)
                .desired_width(ui.available_width()),
        );
        done = field.lost_focus();
    });
    done
}

/// "HH:MM", outlined in the warning color while it does not read as a time.
fn clock_field(ui: &mut Ui, id: &str, text: &mut String) -> bool {
    let valid = rules::parse_clock(text).is_some();
    let mut edit = egui::TextEdit::singleline(text)
        .id(egui::Id::new(id))
        .desired_width(48.0)
        .hint_text("HH:MM");
    if !valid {
        edit = edit.text_color(WARN);
    }
    let response = ui.add(edit);
    response.changed() && rules::parse_clock(text).is_some()
}

/// A short note of what the sliders on a saved rule do not show.
fn rule_extras(rule: &Rule) -> Option<String> {
    let mut notes = Vec::new();
    let finish = &rule.look.finish;
    if finish.tint_amount > 0 || finish.tint_source != TintSource::Custom {
        notes.push("tinted");
    }
    if finish.saturation != mapping::SATURATION_DEFAULT || finish.grain > 0 {
        notes.push("finished");
    }
    match rule.focus {
        FocusLook::Same => {}
        FocusLook::Lighter => notes.push("lighter when focused"),
        FocusLook::Solid => notes.push("solid when focused"),
    }
    if rule.filters_titles() {
        notes.push("some titles");
    }
    (!notes.is_empty()).then(|| notes.join(" · "))
}

fn look_picker(ui: &mut Ui, style: &mut BlurStyle) -> bool {
    let mut changed = false;
    if theme::choice(ui, *style == BlurStyle::Frost, "Frost")
        .on_hover_text("Blur whatever is behind the window. The slider sets how far it spreads.")
        .clicked()
        && *style != BlurStyle::Frost
    {
        *style = BlurStyle::Frost;
        changed = true;
    }
    if theme::choice(ui, *style == BlurStyle::Acrylic, "Acrylic")
        .on_hover_text("Milky system glass. The slider sets how milky it is.")
        .clicked()
        && *style != BlurStyle::Acrylic
    {
        *style = BlurStyle::Acrylic;
        changed = true;
    }
    changed
}

fn labeled_slider(ui: &mut Ui, label: &str, add: impl FnOnce(&mut Ui) -> egui::Response) -> bool {
    let budget = ui.available_width();
    let mut changed = false;
    ui.allocate_ui_with_layout(
        egui::vec2(budget, 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(budget);
            ui.label(theme::muted(label));
            ui.spacing_mut().slider_width = slider_rail_width(ui);
            changed = add(ui).changed();
        },
    );
    changed
}

#[derive(Default)]
struct Heading {
    toggled: bool,
    edit: bool,
    remove: bool,
}

/// The name stays on the left and Edit and Remove stay on the right. A long name
/// ellipsizes instead of pushing the buttons past the window.
fn rule_heading(ui: &mut Ui, enabled: &mut bool, process: &str) -> Heading {
    let row_w = ui.available_width();
    let mut heading = Heading::default();
    ui.horizontal(|ui| {
        ui.set_width(row_w);
        let gap = ui.spacing().item_spacing.x;
        let buttons = button_width(ui, "Edit") + button_width(ui, "Remove") + gap;
        let name_w = (row_w - buttons - gap).max(0.0);
        ui.allocate_ui_with_layout(
            egui::vec2(name_w, 0.0),
            Layout::left_to_right(Align::Center),
            |ui| {
                ui.set_width(name_w);
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                let color = if *enabled { TEXT } else { MUTED };
                heading.toggled = ui
                    .checkbox(enabled, RichText::new(process).strong().color(color))
                    .on_hover_text("Turn this rule on or off.")
                    .changed();
            },
        );
        if ui
            .button("Edit")
            .on_hover_text("Show every setting for this app in the Glass card.")
            .clicked()
        {
            heading.edit = true;
        }
        if ui.button("Remove").clicked() {
            heading.remove = true;
        }
    });
    heading
}

/// Transparency and blur on one line, each in its own half of the row.
fn rule_slider_row(ui: &mut Ui, transparency: &mut u8, blur: &mut u8, milky: bool) -> bool {
    let row_w = ui.available_width();
    let gap = ui.spacing().item_spacing.x;
    // Floor so the two halves plus the gap cannot round past the row and wrap.
    let pair_w = ((row_w - gap) / 2.0).floor().max(0.0);
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.set_width(row_w);
        changed |= slider_pair(ui, "Transparency", pair_w, |ui| {
            ui.add(
                Slider::new(
                    transparency,
                    mapping::TRANSPARENCY_MIN..=mapping::TRANSPARENCY_MAX,
                )
                .suffix("%"),
            )
        });
        changed |= slider_pair(ui, if milky { "Milkiness" } else { "Blur" }, pair_w, |ui| {
            ui.add(Slider::new(blur, mapping::BLUR_MIN..=mapping::BLUR_MAX))
        });
    });
    changed
}

fn button_width(ui: &Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text_w = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
        .x;
    text_w + ui.spacing().button_padding.x * 2.0
}

fn banner(ui: &mut Ui, color: Color32, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(color.gamma_multiply(0.22))
        .stroke(Stroke::new(1.0_f32, color.gamma_multiply(0.5)))
        .corner_radius(10)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add_contents);
        });
    ui.add_space(8.0);
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

fn column_ui(ui: &mut Ui, rect: egui::Rect) -> Ui {
    let mut column = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down_justified(Align::LEFT)),
    );
    column.set_max_width(rect.width());
    column.set_clip_rect(column.clip_rect().intersect(rect));
    column
}

/// A fixed-width label plus slider. The slider uses whatever width is left.
fn slider_pair(
    ui: &mut Ui,
    label: &str,
    width: f32,
    add: impl FnOnce(&mut Ui) -> egui::Response,
) -> bool {
    let width = width.min(ui.available_width()).max(0.0);
    let mut changed = false;
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        Layout::left_to_right(Align::Center),
        |ui| {
            ui.set_width(width);
            ui.label(theme::muted(label).small());
            ui.spacing_mut().slider_width = slider_rail_width(ui);
            changed = add(ui).changed();
        },
    );
    changed
}

/// Width of the number button beside a slider, measured for the widest value.
///
/// `slider_width` is only the rail. The value button is drawn after it, so a rail
/// that uses the whole remaining row paints the number outside the window.
fn slider_value_width(ui: &Ui) -> f32 {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let text = ui
        .painter()
        .layout_no_wrap("100%".to_owned(), font, Color32::WHITE)
        .size()
        .x;
    let padded = text + ui.spacing().button_padding.x * 2.0;
    padded.max(ui.spacing().interact_size.x) + 4.0
}

/// Rail width that leaves room for the value button in the space still free.
fn slider_rail_width(ui: &Ui) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    (ui.available_width() - gap - slider_value_width(ui)).max(0.0)
}

enum RowBadge {
    Pill(String, Color32),
    Count(String),
}

fn row_badges(group: &AppGroup, rule: Option<&Rule>) -> Vec<RowBadge> {
    let mut badges = Vec::new();
    if group.elevated {
        badges.push(RowBadge::Pill("Admin".to_string(), WARN));
    }
    if group.fullscreen {
        badges.push(RowBadge::Pill("Fullscreen".to_string(), WARN));
    }
    if let Some(rule) = rule {
        let look = match rule.look.style {
            BlurStyle::Frost => "Frost",
            BlurStyle::Acrylic => "Acrylic",
        };
        badges.push(RowBadge::Pill(
            format!("{look} · {}%", rule.look.transparency),
            ACCENT,
        ));
    }
    badges.push(RowBadge::Count(format!(
        "{} window{}",
        group.windows,
        if group.windows == 1 { "" } else { "s" }
    )));
    badges
}

fn draw_badge(ui: &mut Ui, badge: &RowBadge) {
    match badge {
        RowBadge::Pill(text, color) => theme::status_pill(ui, text, *color),
        RowBadge::Count(text) => {
            ui.label(theme::muted(text).small());
        }
    }
}

fn badge_width(ui: &Ui, badge: &RowBadge) -> f32 {
    match badge {
        // Pill frame: 8px padding on each side, plus a little slack so the
        // one-line layout wraps before the badges cross the row.
        RowBadge::Pill(text, _) => text_width(ui, text, 12.0) + 24.0,
        RowBadge::Count(text) => {
            text_width(ui, text, egui::TextStyle::Small.resolve(ui.style()).size)
        }
    }
}

fn text_width(ui: &Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            Color32::WHITE,
        )
        .size()
        .x
}

fn name_block(ui: &mut Ui, group: &AppGroup, width: f32) {
    ui.vertical(|ui| {
        ui.set_max_width(width.max(0.0));
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
        ui.spacing_mut().item_spacing.y = 1.0;
        ui.label(RichText::new(&group.process).strong().color(TEXT));
        ui.add(egui::Label::new(theme::muted(&group.sample_title).small()).truncate());
    });
}

fn app_row(ui: &mut Ui, group: &AppGroup, selected: bool, rule: Option<&Rule>) -> egui::Response {
    let background = ui.painter().add(Shape::Noop);
    let inner = Frame::new()
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            let row_w = ui.available_width();
            ui.set_max_width(row_w);
            let badges = row_badges(group, rule);
            let gap = ui.spacing().item_spacing.x;
            let badges_w = badges
                .iter()
                .map(|badge| badge_width(ui, badge))
                .sum::<f32>()
                + gap * badges.len().saturating_sub(1) as f32
                + 12.0;
            // Keep the name on the same line when the badges leave it room.
            if badges_w + gap + 96.0 <= row_w {
                ui.horizontal(|ui| {
                    ui.set_max_width(row_w);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for badge in badges.iter().rev() {
                            draw_badge(ui, badge);
                        }
                        name_block(ui, group, ui.available_width());
                    });
                });
            } else {
                name_block(ui, group, row_w);
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(row_w);
                    for badge in &badges {
                        draw_badge(ui, badge);
                    }
                });
            }
        });
    let rect = inner.response.rect;
    let mut response = ui
        .interact(rect, ui.id().with(("app", &group.process)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand);
    if group.fullscreen {
        response =
            response.on_hover_text("This app covers the screen, so Blurman leaves it alone.");
    }
    let fill = if selected {
        ROW_SELECTED
    } else if response.hovered() {
        ROW_HOVER
    } else {
        Color32::TRANSPARENT
    };
    ui.painter()
        .set(background, Shape::rect_filled(rect, 8, fill));
    response
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
    decode_icon(include_bytes!("../assets/icon-256.png"))
}

pub fn tray_icon() -> egui::IconData {
    decode_icon(include_bytes!("../assets/icon-32.png"))
}

#[cfg(test)]
mod tests {
    use super::{
        app_row, finish_controls, keep_unsaved_looks, labeled_slider, rule_heading, slider_pair,
    };
    use crate::mapping::{BlurStyle, FocusLook, Look};
    use crate::rules::{self, Store};
    use crate::target::AppGroup;
    use crate::theme;
    use egui::{Slider, Ui};

    fn look(transparency: u8, blur: u8, style: BlurStyle) -> Look {
        Look {
            transparency,
            blur,
            style,
            ..Look::default()
        }
    }

    #[test]
    fn an_unsaved_drag_survives_a_reload_and_the_file_wins_the_rest() {
        let mut local = Store::default();
        local.upsert(rules::new_rule("notepad", look(55, 70, BlurStyle::Acrylic)));
        local.upsert(rules::new_rule("gone", look(30, 40, BlurStyle::Frost)));
        // The tray paused everything from an older copy of the file, and the CLI added chrome.
        let mut disk = Store {
            paused: true,
            ..Store::default()
        };
        let mut notepad = rules::new_rule("notepad", look(30, 40, BlurStyle::Frost));
        notepad.focus = FocusLook::Solid;
        disk.upsert(notepad);
        disk.upsert(rules::new_rule("chrome", look(20, 10, BlurStyle::Frost)));
        keep_unsaved_looks(&mut disk, &local);
        assert!(disk.paused);
        assert_eq!(disk.rules.len(), 2);
        let notepad = disk.rule("notepad").unwrap();
        assert_eq!(notepad.look, look(55, 70, BlurStyle::Acrylic));
        // Behavior is written at once, so the file is right about it.
        assert_eq!(notepad.focus, FocusLook::Solid);
        assert_eq!(disk.rule("chrome").unwrap().look.transparency, 20);
        assert!(disk.rule("gone").is_none());
    }

    #[test]
    fn finish_sliders_stay_inside_a_narrow_column() {
        let overflow = sticks_out(340.0, |ui| {
            let mut look = Look::default();
            // Open the section so its rows are laid out.
            let id = ui.make_persistent_id("finish");
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .store(ui.ctx());
            finish_controls(ui, &mut look, false);
        });
        assert!(overflow <= 1.0, "finish sliders stick out by {overflow}px");
    }

    /// How far `add` paints outside a region `width` points wide.
    fn sticks_out(width: f32, add: impl FnOnce(&mut Ui)) -> f32 {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(width, 800.0),
        ));
        let mut add = Some(add);
        let mut overflow = 0.0_f32;
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    let bounds = ui.max_rect();
                    let inner = ui.scope(|ui| {
                        if let Some(add) = add.take() {
                            add(ui);
                        }
                    });
                    let rect = inner.response.rect;
                    overflow = (rect.right() - bounds.right())
                        .max(bounds.left() - rect.left())
                        .max(0.0);
                });
        });
        overflow
    }

    #[test]
    fn slider_rows_stay_inside_a_narrow_column() {
        let overflow = sticks_out(340.0, |ui| {
            let mut transparency = 70_u8;
            let mut blur = 100_u8;
            labeled_slider(ui, "Transparency", |ui| {
                ui.add(Slider::new(&mut transparency, 10..=70).suffix("%"))
            });
            labeled_slider(ui, "Milkiness", |ui| {
                ui.add(Slider::new(&mut blur, 1..=100))
            });
        });
        assert!(overflow <= 1.0, "glass sliders stick out by {overflow}px");
    }

    #[test]
    fn saved_rule_sliders_share_one_line() {
        let ctx = egui::Context::default();
        theme::apply(&ctx);
        let mut raw = egui::RawInput::default();
        raw.screen_rect = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1000.0, 400.0),
        ));
        let mut overflow = 0.0_f32;
        let mut same_line = false;
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::new())
                .show(ctx, |ui| {
                    let bounds = ui.max_rect();
                    let mut transparency = 10_u8;
                    let mut blur = 40_u8;
                    let mut tops = [0.0_f32, 0.0];
                    let inner = ui.scope(|ui| {
                        let row_w = ui.available_width();
                        let gap = ui.spacing().item_spacing.x;
                        let pair_w = ((row_w - gap) / 2.0).floor().max(0.0);
                        ui.horizontal(|ui| {
                            ui.set_width(row_w);
                            tops[0] = ui.cursor().top();
                            slider_pair(ui, "Transparency", pair_w, |ui| {
                                ui.add(Slider::new(&mut transparency, 10..=70).suffix("%"))
                            });
                            tops[1] = ui.cursor().top();
                            slider_pair(ui, "Blur", pair_w, |ui| {
                                ui.add(Slider::new(&mut blur, 1..=100))
                            });
                        });
                    });
                    let rect = inner.response.rect;
                    overflow = (rect.right() - bounds.right())
                        .max(bounds.left() - rect.left())
                        .max(0.0);
                    same_line = (tops[0] - tops[1]).abs() < 1.0;
                });
        });
        assert!(
            overflow <= 1.0,
            "saved-rule sliders stick out by {overflow}px"
        );
        assert!(same_line, "transparency and blur wrapped onto two lines");
    }

    #[test]
    fn long_rule_name_keeps_remove_inside() {
        let overflow = sticks_out(320.0, |ui| {
            let mut enabled = true;
            rule_heading(
                ui,
                &mut enabled,
                "ApplicationFrameHost.exe with a very long window name",
            );
        });
        assert!(overflow <= 1.0, "rule heading sticks out by {overflow}px");
    }

    #[test]
    fn app_rows_stay_inside_the_list() {
        let group = AppGroup {
            process: "ApplicationFrameHost.exe".to_string(),
            sample_title: "A very long window title that should stay inside the row".to_string(),
            windows: 12,
            elevated: true,
            fullscreen: true,
        };
        let rule = rules::new_rule(&group.process, look(70, 100, BlurStyle::Acrylic));
        let overflow = sticks_out(340.0, |ui| {
            app_row(ui, &group, true, Some(&rule));
        });
        assert!(overflow <= 1.0, "app row sticks out by {overflow}px");
    }
}
