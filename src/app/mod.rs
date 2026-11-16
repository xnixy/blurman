//! The Blurman window: a frameless window of islands. The header pill is its title bar, the
//! sidebar holds the pages, and each island sits on glass where Windows composes it.

mod apps;
pub(crate) mod glass_theme;
pub(crate) mod look;
mod pages;
mod panel;
mod widgets;

use crate::autostart;
use crate::ipc;
use crate::mapping::{FocusLook, Look};
use crate::picker;
use crate::rules::{self, HotkeyAction, Rule, Settings, Store};
use crate::shared::{Shared, Tweak};
use crate::system;
use crate::target::{self, AppGroup};
use crate::theme::{self, colors, Mode};
use crate::tray;
use egui::{Align, Layout, Pos2, Rect, RichText, Sense, Ui, UiBuilder};
use panel::GlassPanel;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use widgets::{Icon, IconCache};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_TRANSIENTWINDOW,
    DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE, DWMWA_WINDOW_CORNER_PREFERENCE,
    DWMWCP_DONOTROUND, DWMWCP_ROUND, DWMWINDOWATTRIBUTE, DWMWA_BORDER_COLOR,
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
const SIDEBAR_WIDTH: f32 = 220.0;
/// The sidebar as a rail of icons: when asked for, and whenever the window is narrow.
const RAIL_WIDTH: f32 = 82.0;
const RAIL_BELOW: f32 = 720.0;
/// Between the window's edge and the islands, and between islands.
const GAP: f32 = 12.0;
const HEADER_HEIGHT: f32 = 46.0;
/// With less room than this, a page of two panes shows one at a time.
const ONE_PANE_BELOW: f32 = 620.0;
const WINDOW_SIZE: [f32; 2] = [1080.0, 720.0];
const MIN_SIZE: [f32; 2] = [560.0, 420.0];
/// The band along each edge that resizes the window, and its longer reach at the corners.
const RESIZE_EDGE: f32 = 6.0;
const RESIZE_CORNER: f32 = 12.0;
/// How long after opening the window is checked against its monitor.
const PLACE_FOR: Duration = Duration::from_secs(2);

/// The window is closing only to be made again, in another style.
static REOPEN: AtomicBool = AtomicBool::new(false);
/// The page the window was on when it closed to be made again, as an index into `Page::ALL`.
static REOPEN_ON: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

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
        if REOPEN.swap(false, Ordering::SeqCst) {
            in_tray = false;
            continue;
        }
        REOPEN_ON.store(0, Ordering::SeqCst);
        if !shared.to_tray.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        in_tray = true;
    }
}

fn open_window(shared: &Arc<Shared>, tray_session: bool) -> Result<(), String> {
    let settings = rules::load_settings();
    let scale = theme::text_scale();
    let wanted = Mode::now(&settings.appearance.window);
    // eframe keeps window size and position in a folder named after the app, so a test
    // profile gets its own name and leaves yours alone.
    let app_name = ipc::scoped("Blurman");
    // Left to Windows, a frameless window lands in the top-left corner. Once it has been
    // somewhere, it comes back there.
    let placed_before = eframe::storage_dir(&app_name)
        .map(|dir| dir.join("app.ron").exists())
        .unwrap_or(false);
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Blurman")
            .with_inner_size([WINDOW_SIZE[0] * scale, WINDOW_SIZE[1] * scale])
            .with_min_inner_size([MIN_SIZE[0] * scale, MIN_SIZE[1] * scale])
            .with_decorations(false)
            .with_transparent(matches!(wanted, Mode::Glass | Mode::Frosted | Mode::Islands))
            .with_icon(window_icon()),
        centered: !placed_before,
        ..Default::default()
    };
    let app_shared = shared.clone();
    let result = eframe::run_native(
        &app_name,
        options,
        Box::new(move |cc| {
            let ctx = &cc.egui_ctx;
            theme::install_fonts(ctx);
            // Windows' text size is the zoom. Ctrl and plus would fight it.
            ctx.set_zoom_factor(scale);
            ctx.options_mut(|options| options.zoom_with_keyboard = false);
            follow_theme_setting(ctx, &settings.appearance.theme);
            app_shared.set_ctx(Some(ctx.clone()));
            let light = ctx.theme() == egui::Theme::Light;
            let accent = settings.appearance.accent.clone();
            // The glass is made in the colors of the mode it opens in.
            theme::set(light, &accent, wanted, false);
            let mut mode = if wanted == Mode::HighContrast {
                wanted
            } else {
                Mode::Solid
            };
            let mut panel = None;
            if let Ok(handle) = cc.window_handle() {
                if let RawWindowHandle::Win32(win32) = handle.as_raw() {
                    let hwnd = win32.hwnd.get();
                    app_shared.main_window.store(hwnd, Ordering::SeqCst);
                    (mode, panel) = make_backdrop(hwnd, wanted, light);
                }
            }
            theme::set(light, &accent, mode, panel.is_some());
            theme::restyle(ctx);
            #[allow(unused_mut)]
            let mut app = BlurmanApp::new(
                app_shared,
                settings,
                tray_session,
                Backdrop {
                    wanted,
                    mode,
                    panel,
                    scale,
                },
            );
            #[cfg(debug_assertions)]
            app.preview_from_env(ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|err| err.to_string());
    shared.main_window.store(0, Ordering::SeqCst);
    shared.set_ctx(None);
    // The window can close into the tray while a shortcut is still recording.
    if shared.recording_shortcut.swap(false, Ordering::SeqCst) {
        ipc::signal(ipc::msg_settings());
    }
    result
}

/// What is behind the islands, decided when the window is made.
struct Backdrop {
    /// What the settings and Windows asked for.
    wanted: Mode,
    /// What this PC could do of it.
    mode: Mode,
    /// The glass under the islands, in `Mode::Glass`. It goes away with the window.
    panel: Option<GlassPanel>,
    /// Windows' text size when the window was made.
    scale: f32,
}

/// Light, dark, or as Windows has it. BLURMAN_THEME tries one without changing anything.
fn follow_theme_setting(ctx: &egui::Context, setting: &str) {
    let tried = std::env::var("BLURMAN_THEME").ok();
    let preference = match tried.as_deref().unwrap_or(setting) {
        "light" => egui::ThemePreference::Light,
        "dark" => egui::ThemePreference::Dark,
        _ => egui::ThemePreference::System,
    };
    if ctx.options(|options| options.theme_preference) != preference {
        ctx.set_theme(preference);
    }
}

/// Put glass under the islands, or frost the whole window, or neither: the nearest to `wanted`
/// that this PC can do. BLURMAN_NO_GLASS skips the glass, to try the window without it.
fn make_backdrop(hwnd: isize, wanted: Mode, light: bool) -> (Mode, Option<GlassPanel>) {
    let window = HWND(hwnd as *mut _);
    if wanted == Mode::Glass && std::env::var_os("BLURMAN_NO_GLASS").is_none() {
        let panel = GlassPanel::attach(hwnd, panel::Style::from_theme(theme::ISLAND_RADIUS as f32));
        if panel.composed() {
            return (Mode::Glass, Some(panel));
        }
    }
    no_system_menu(window);
    if wanted == Mode::Islands {
        // Invisible around the islands: no Windows corners or border on the window itself.
        let _ = set_dwm_u32(window, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
        // DWMWA_COLOR_NONE: no border at all.
        let _ = set_dwm_u32(window, DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
        return (Mode::Islands, None);
    }
    let _ = set_dwm_u32(
        window,
        DWMWA_WINDOW_CORNER_PREFERENCE,
        DWMWCP_ROUND.0 as u32,
    );
    match wanted {
        Mode::Glass | Mode::Frosted if frost_window(window, light) => (Mode::Frosted, None),
        Mode::HighContrast => (Mode::HighContrast, None),
        _ => (Mode::Solid, None),
    }
}

/// Ask Windows again for no border and no rounded corners on the window. Where only the islands
/// show, the window itself must stay invisible, and a shell that styles other apps' frames
/// (shellman's does) gives it a colored border and round corners after it opens.
fn keep_bare(window: HWND) {
    if window.0.is_null() {
        return;
    }
    let _ = set_dwm_u32(window, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_DONOTROUND.0 as u32);
    // DWMWA_COLOR_NONE: no border at all.
    let _ = set_dwm_u32(window, DWMWA_BORDER_COLOR, 0xFFFF_FFFE);
}

fn set_dwm_u32(hwnd: HWND, attribute: DWMWINDOWATTRIBUTE, value: u32) -> windows::core::Result<()> {
    unsafe {
        DwmSetWindowAttribute(
            hwnd,
            attribute,
            (&raw const value).cast(),
            size_of::<u32>() as u32,
        )
    }
}

/// Give the window the Windows 11 acrylic backdrop, dark or light. False on Windows versions
/// without system backdrops, where the window stays opaque.
fn frost_window(hwnd: HWND, light: bool) -> bool {
    let _ = set_dwm_u32(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!light));
    let margins = MARGINS {
        cxLeftWidth: -1,
        cxRightWidth: -1,
        cyTopHeight: -1,
        cyBottomHeight: -1,
    };
    let frosted = unsafe { DwmExtendFrameIntoClientArea(hwnd, &margins) }.is_ok()
        && set_dwm_u32(
            hwnd,
            DWMWA_SYSTEMBACKDROP_TYPE,
            DWMSBT_TRANSIENTWINDOW.0 as u32,
        )
        .is_ok();
    if frosted {
        unsafe {
            let _ = SetWindowSubclass(hwnd, Some(stay_frosted), 1, 0);
        }
    }
    frosted
}

/// The header draws the window's buttons. winit keeps a system menu on its frameless windows,
/// and sets it again whenever it restyles them, and Windows then draws faint caption buttons of
/// its own in the corner of a frosted window. This takes the system menu out of the style, now
/// and in every later restyle.
fn no_system_menu(hwnd: HWND) {
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_STYLE, SWP_FRAMECHANGED,
        SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_SYSMENU,
    };
    unsafe {
        let _ = SetWindowSubclass(hwnd, Some(strip_system_menu), 2, 0);
        let style = GetWindowLongPtrW(hwnd, GWL_STYLE) & !(WS_SYSMENU.0 as isize);
        SetWindowLongPtrW(hwnd, GWL_STYLE, style);
        let _ = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            0,
            0,
            SWP_FRAMECHANGED | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
        );
    }
}

unsafe extern "system" fn strip_system_menu(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::{
        GWL_STYLE, STYLESTRUCT, WM_STYLECHANGING, WS_SYSMENU,
    };
    // For WM_STYLECHANGING with GWL_STYLE, `lparam` points to the style about to be set.
    if msg == WM_STYLECHANGING && wparam.0 as i32 == GWL_STYLE.0 {
        (*(lparam.0 as *mut STYLESTRUCT)).styleNew &= !WS_SYSMENU.0;
    }
    DefSubclassProc(hwnd, msg, wparam, lparam)
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

/// Which edge or corner of the window `at` is on, for resizing a window with no frame.
fn resize_edge(window: Rect, at: Pos2) -> Option<egui::ResizeDirection> {
    use egui::ResizeDirection::*;
    if !window.contains(at) {
        return None;
    }
    let left = at.x - window.left();
    let right = window.right() - at.x;
    let top = at.y - window.top();
    let bottom = window.bottom() - at.y;
    if left.min(right).min(top).min(bottom) >= RESIZE_EDGE {
        return None;
    }
    let near = |distance: f32| distance < RESIZE_CORNER;
    Some(match (near(top), near(bottom), near(left), near(right)) {
        (true, _, true, _) => NorthWest,
        (true, _, _, true) => NorthEast,
        (_, true, true, _) => SouthWest,
        (_, true, _, true) => SouthEast,
        _ if top < RESIZE_EDGE => North,
        _ if bottom < RESIZE_EDGE => South,
        _ if left < RESIZE_EDGE => West,
        _ => East,
    })
}

fn resize_cursor(direction: egui::ResizeDirection) -> egui::CursorIcon {
    use egui::{CursorIcon, ResizeDirection::*};
    match direction {
        North | South => CursorIcon::ResizeVertical,
        East | West => CursorIcon::ResizeHorizontal,
        NorthWest | SouthEast => CursorIcon::ResizeNwSe,
        NorthEast | SouthWest => CursorIcon::ResizeNeSw,
    }
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

    fn icon(self) -> Icon {
        match self {
            Page::Apps => Icon::APPS,
            Page::Presets => Icon::PRESETS,
            Page::Shortcuts => Icon::SHORTCUTS,
            Page::AutoPause => Icon::AUTO_PAUSE,
            Page::Settings => Icon::SETTINGS,
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
    /// settings.json as the window last read or wrote it, to tell an outside change from its own.
    settings_on_disk: Settings,
    seen_settings_generation: u64,
    autostart: bool,
    tray_error: Option<String>,
    seen_generation: u64,
    groups: Vec<AppGroup>,
    scanned: Instant,
    page: Page,
    selected: Option<String>,
    /// In a window too narrow for two panes: the editor shows, not the list.
    detail_open: bool,
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
    logo: Option<egui::TextureHandle>,
    /// Launched by Windows startup: this session lives in the tray whatever the setting says.
    startup: bool,
    backdrop: Backdrop,
    /// The mode and accent the colors were last made for.
    drawn_for: Option<(bool, egui::Color32)>,
    /// The pointer was on a resize edge, so the cursor is put back when it leaves.
    on_edge: bool,
    /// The 150% display was painting into a window sized as if it were 100%.
    dpi_matched: bool,
    /// When a slider edit should be written. None while the file matches the sliders.
    pending_save: Option<Instant>,
    /// When the window opened. For a moment after, it is kept on screen.
    opened: Instant,
}

impl BlurmanApp {
    fn new(shared: Arc<Shared>, settings: Settings, startup: bool, backdrop: Backdrop) -> Self {
        let store = rules::load();
        let tray_error = tray::sync(settings.close_to_tray || startup, &store).err();
        Self {
            shared,
            store,
            settings_on_disk: settings.clone(),
            settings,
            seen_settings_generation: 0,
            autostart: autostart::is_enabled(),
            tray_error,
            seen_generation: 0,
            groups: target::list_groups(),
            scanned: Instant::now(),
            page: Page::ALL[REOPEN_ON.swap(0, Ordering::SeqCst) % Page::ALL.len()],
            selected: None,
            detail_open: false,
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
            logo: None,
            startup,
            backdrop,
            drawn_for: None,
            on_edge: false,
            dpi_matched: false,
            pending_save: None,
            opened: Instant::now(),
        }
    }

    /// Debug builds only: open on a given page, app, tab, dialog, or size, so each screen can
    /// be checked without clicking through the window.
    #[cfg(debug_assertions)]
    fn preview_from_env(&mut self, ctx: &egui::Context) {
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
        // "600x700", in points.
        if let Some(size) = var("BLURMAN_UI_SIZE") {
            let mut sides = size.split('x').filter_map(|side| side.parse::<f32>().ok());
            if let (Some(width), Some(height)) = (sides.next(), sides.next()) {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(width, height)));
            }
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
        // A file that cannot be read just now is tried again on the next frame, not taken for
        // an empty one that the next save would write over the rules.
        let Ok(mut store) = rules::read() else {
            return;
        };
        self.seen_generation = generation;
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

    /// Pick up presets, shortcuts, and auto-pause changed from outside, as `blurman import` does.
    /// Without this the window's next save would write its older copy over them.
    fn sync_settings_from_disk(&mut self) {
        let generation = self.shared.settings_generation.load(Ordering::SeqCst);
        if generation == self.seen_settings_generation {
            return;
        }
        self.seen_settings_generation = generation;
        // A file that is missing or cannot be read is not a change to pick up: taking the
        // defaults for it would have the next save write them over the user's presets.
        let Some(loaded) = rules::read_settings() else {
            return;
        };
        // The window's own saves come back through here too. Only a file something else changed
        // replaces what the window holds, so a field being typed in is not reset.
        if loaded != self.settings_on_disk {
            self.settings = loaded.clone();
            self.settings_on_disk = loaded;
            self.recording = None;
            self.renaming = None;
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
        // What was being typed for the app shown until now is kept, as leaving its field would.
        self.save_typed();
        self.page = Page::Apps;
        self.selected = Some(process);
        self.detail_open = true;
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

    /// Tell the effect thread when a shortcut starts or stops recording, so it lets the
    /// shortcuts go for that long.
    fn share_recording(&self) {
        let recording = self.recording.is_some();
        if self
            .shared
            .recording_shortcut
            .swap(recording, Ordering::SeqCst)
            != recording
        {
            ipc::signal(ipc::msg_settings());
        }
    }

    /// Whether the title filters typed for the selected app differ from its saved rule.
    fn titles_unsaved(&self) -> bool {
        self.selected_rule().is_some_and(|rule| {
            rule.only_titles != rules::split_titles(&self.only_titles)
                || rule.skip_titles != rules::split_titles(&self.skip_titles)
        })
    }

    /// Save what is still being typed, as leaving the field would have. A field is only seen
    /// to lose the focus if it is drawn again, and it is not when the window closes or the
    /// click that took the focus also changed the page.
    fn save_typed(&mut self) {
        if self.titles_unsaved() {
            self.behavior_to_rule();
        }
        let last = self.settings_on_disk.auto_pause.clone();
        self.settings.auto_pause.keep_times_that_read(&last);
        if self.settings != self.settings_on_disk {
            self.save_settings();
        }
    }

    fn save_settings(&mut self) {
        // Another setting can be saved while a time is half typed. The file keeps the last time
        // that read as one, or the schedule would stop working with nothing to show why.
        let last = self.settings_on_disk.auto_pause.clone();
        self.settings.auto_pause.keep_times_that_read(&last);
        match rules::save_settings(&self.settings) {
            Ok(()) => {
                self.settings_on_disk = self.settings.clone();
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
        // The size in the display's own points: egui's points are larger by the text size.
        let points = ctx.screen_rect().size() * ctx.zoom_factor();
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

    /// Keep the colors, egui's style, and the glass in step with the theme and the accent,
    /// whether the setting changed or Windows did.
    fn follow_look(&mut self, ctx: &egui::Context) {
        follow_theme_setting(ctx, &self.settings.appearance.theme);
        let light = ctx.theme() == egui::Theme::Light;
        let accent = theme::accent_for(&self.settings.appearance.accent, light);
        if self.drawn_for == Some((light, accent)) {
            return;
        }
        self.drawn_for = Some((light, accent));
        theme::set(
            light,
            &self.settings.appearance.accent,
            self.backdrop.mode,
            self.backdrop.panel.is_some(),
        );
        theme::restyle(ctx);
        if let Some(panel) = &self.backdrop.panel {
            panel.set_style(panel::Style::from_theme(theme::ISLAND_RADIUS as f32));
        }
        if self.backdrop.mode == Mode::Frosted {
            let hwnd = target::hwnd_of(self.shared.main_window.load(Ordering::SeqCst));
            if !hwnd.0.is_null() {
                let _ = set_dwm_u32(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, u32::from(!light));
            }
        }
    }

    /// Make the window again when what it was made for has changed: the window style, high
    /// contrast, Windows' transparency switch, a Remote Desktop session, or the text size.
    /// It comes back in the same place.
    fn remake_if_needed(&mut self, ctx: &egui::Context) {
        let wanted = Mode::now(&self.settings.appearance.window);
        if wanted != self.backdrop.wanted || theme::text_scale() != self.backdrop.scale {
            // Asked once: the window takes a frame or two to close.
            self.backdrop.wanted = wanted;
            self.backdrop.scale = theme::text_scale();
            let page = Page::ALL.iter().position(|page| *page == self.page);
            REOPEN_ON.store(page.unwrap_or(0), Ordering::SeqCst);
            REOPEN.store(true, Ordering::SeqCst);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    /// Step aside so any window can be clicked to choose its app.
    fn pick_window(&mut self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        picker::start(self.shared.clone());
    }

    fn missing_builtin_presets(&self) -> usize {
        rules::default_presets()
            .into_iter()
            .filter(|preset| self.settings.preset(&preset.name).is_none())
            .count()
    }

    fn restore_builtin_presets(&mut self) {
        for preset in rules::default_presets() {
            if self.settings.preset(&preset.name).is_none() {
                self.settings.presets.push(preset);
            }
        }
        self.save_settings();
        self.notify("The built-in presets are back");
    }

    /// The window has no frame, so its edges are resized here: a resize cursor over the band
    /// along each edge, and a press there hands the resize to Windows.
    fn resize_edges(&mut self, ctx: &egui::Context) {
        let (at, pressed, maximized) = ctx.input(|input| {
            (
                input.pointer.hover_pos(),
                input.pointer.primary_pressed(),
                input.viewport().maximized.unwrap_or(false),
            )
        });
        let edge = at
            .filter(|_| !maximized)
            .and_then(|at| resize_edge(ctx.screen_rect(), at));
        match edge {
            Some(direction) => {
                self.on_edge = true;
                ctx.set_cursor_icon(resize_cursor(direction));
                if pressed {
                    ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                }
            }
            // egui only sets the pointer when its own choice changes, so the arrows would
            // stay after leaving the band.
            None if self.on_edge => {
                self.on_edge = false;
                ctx.set_cursor_icon(egui::CursorIcon::Default);
            }
            None => {}
        }
    }

    /// Any part of the window with no control on it moves the window: the header, the gaps
    /// between islands, and the empty space inside the sidebar and the panes. A double-click
    /// maximizes it. This is made before the islands, so their controls take presses first.
    fn window_drag(&self, ui: &mut Ui, window: Rect) {
        let response = ui.interact(
            window,
            egui::Id::new("window-drag"),
            Sense::CLICK | Sense::DRAG,
        );
        widgets::name(ui, &response, "Blurman");
        // Where the press began. A click has let go by the time it is seen, and has only where
        // the pointer is.
        let origin = ui.input(|input| {
            input
                .pointer
                .press_origin()
                .or(input.pointer.interact_pos())
        });
        let on_handle = origin.is_some_and(|at| resize_edge(window, at).is_none());
        if !on_handle {
            return;
        }
        if response.double_clicked() {
            let maximized = ui.input(|input| input.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        } else if response.drag_started_by(egui::PointerButton::Primary) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    /// The whole window: the sidebar island, the header pill, and the page under it.
    fn shell(&mut self, ui: &mut Ui) {
        let window = ui.max_rect();
        let rail = self.settings.appearance.compact_sidebar || window.width() < RAIL_BELOW;
        let side = Rect::from_min_max(
            window.min + egui::vec2(GAP, GAP),
            egui::pos2(
                window.left() + GAP + if rail { RAIL_WIDTH } else { SIDEBAR_WIDTH },
                window.bottom() - GAP,
            ),
        );
        let left = side.right() + GAP;
        let head = Rect::from_min_max(
            egui::pos2(left, window.top() + GAP),
            egui::pos2(window.right() - GAP, window.top() + GAP + HEADER_HEIGHT),
        );
        let body = Rect::from_min_max(
            egui::pos2(left, head.bottom() + GAP),
            egui::pos2(window.right() - GAP, window.bottom() - GAP),
        );
        self.window_drag(ui, window);
        self.sidebar(ui, side, rail);
        self.header(ui, head);
        let mut page = ui.new_child(
            UiBuilder::new()
                .max_rect(body)
                .layout(Layout::top_down(Align::LEFT)),
        );
        page.set_clip_rect(body.intersect(ui.clip_rect()));
        self.banners(&mut page);
        match self.page {
            Page::Apps => {
                let rest = page.available_rect_before_wrap();
                self.apps_page(&mut page, rest);
            }
            Page::Presets => self.presets_page(&mut page),
            Page::Shortcuts => column(&mut page, "shortcuts", |ui| self.shortcuts_page(ui)),
            Page::AutoPause => column(&mut page, "auto-pause", |ui| self.auto_pause_page(ui)),
            Page::Settings => column(&mut page, "settings", |ui| self.settings_page(ui)),
        }
    }

    /// The title bar: "Blurman / Page", the page's search and buttons, and the window buttons.
    fn header(&mut self, ui: &mut Ui, rect: Rect) {
        widgets::island(ui, rect, rect.height() / 2.0);
        let inner = Rect::from_min_max(
            rect.min + egui::vec2(18.0, 0.0),
            rect.max - egui::vec2(8.0, 0.0),
        );
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::right_to_left(Align::Center)),
        );
        ui.set_clip_rect(inner.intersect(ui.clip_rect()));
        ui.spacing_mut().item_spacing.x = 2.0;
        let ctx = ui.ctx().clone();
        let close = if self.keeps_in_tray() {
            "Close to the tray"
        } else {
            "Close"
        };
        if widgets::window_button(&mut ui, Icon::CLOSE, close).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
        let (icon, name) = if maximized {
            (Icon::RESTORE, "Restore")
        } else {
            (Icon::MAXIMIZE, "Maximize")
        };
        if widgets::window_button(&mut ui, icon, name).clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if widgets::window_button(&mut ui, Icon::MINIMIZE, "Minimize").clicked() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }

        let c = colors();
        let page_font = theme::semibold(14.0);
        let page_w = widgets::text_width_in(&ui, self.page.label(), page_font.clone()) + 24.0;
        let name_w = widgets::text_width_in(&ui, "Blurman", theme::semibold(17.0)) + 26.0;
        // The page's own buttons, only where they leave room for its name.
        ui.add_space(8.0);
        if ui.available_width() > page_w + 40.0 {
            match self.page {
                Page::Apps => {
                    if widgets::round_icon(&mut ui, Icon::PICK, "Pick a window").clicked() {
                        self.pick_window(&ctx);
                    }
                }
                Page::Presets
                    if self.missing_builtin_presets() > 0
                        && widgets::round_icon(
                            &mut ui,
                            Icon::RESTORE_PRESETS,
                            "Add the built-in presets back",
                        )
                        .clicked() =>
                {
                    self.restore_builtin_presets();
                }
                _ => {}
            }
        }
        if self.page == Page::Apps {
            let room = ui.available_width() - page_w - name_w - 16.0;
            if room < 150.0 {
                // With no field to clear it in, a search would hide apps for no visible reason.
                self.search.clear();
            } else {
                ui.add_space(6.0);
                let field = ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .hint_text("Search apps")
                        .margin(widgets::FIELD_MARGIN)
                        .desired_width(room.min(260.0) - 24.0),
                );
                widgets::name(&ui, &field, "Search apps");
            }
        }
        let show_name = ui.available_width() >= page_w + name_w;
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if show_name {
                ui.label(
                    RichText::new("Blurman")
                        .font(theme::semibold(17.0))
                        .color(c.text),
                );
                ui.label(RichText::new("/").size(15.0).color(c.muted));
            }
            let (chip, _) = ui.allocate_exact_size(egui::vec2(page_w, 26.0), Sense::hover());
            ui.painter()
                .rect_filled(chip, egui::CornerRadius::same(13), c.accent);
            ui.painter().text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                self.page.label(),
                page_font,
                c.on_accent,
            );
        });
    }

    /// The app's name and what it is doing, the pages, and the pause control.
    fn sidebar(&mut self, ui: &mut Ui, rect: Rect, rail: bool) {
        widgets::island(ui, rect, theme::ISLAND_RADIUS as f32);
        let inner = rect.shrink(10.0);
        let mut ui = ui.new_child(
            UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::top_down(Align::LEFT)),
        );
        ui.set_clip_rect(inner.intersect(ui.clip_rect()));
        let ui = &mut ui;
        let c = colors();
        // A narrow window has the rail whatever the setting says.
        let forced = !self.settings.appearance.compact_sidebar && rail;
        let menu = |ui: &mut Ui, compact: bool| {
            ui.add_enabled_ui(!forced, |ui| {
                let name = if compact {
                    "Show page names"
                } else {
                    "Compact sidebar"
                };
                widgets::round_icon(ui, Icon::MENU, name).clicked()
            })
            .inner
        };
        let mut toggle = false;
        if rail {
            ui.vertical_centered(|ui| toggle = menu(ui, true));
        } else {
            let status = self.status_line();
            ui.horizontal(|ui| {
                ui.add_space(4.0);
                let logo = self.logo.get_or_insert_with(|| {
                    let icon = window_icon();
                    let image = egui::ColorImage::from_rgba_unmultiplied(
                        [icon.width as usize, icon.height as usize],
                        &icon.rgba,
                    );
                    ui.ctx()
                        .load_texture("logo", image, egui::TextureOptions::LINEAR)
                });
                ui.add(egui::Image::new((logo.id(), egui::vec2(24.0, 24.0))).alt_text("Blurman"));
                ui.label(
                    RichText::new("Blurman")
                        .font(theme::semibold(15.0))
                        .color(c.text),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    toggle = menu(ui, false);
                });
            });
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                let (text, color) = status;
                let label = ui
                    .add(egui::Label::new(RichText::new(text).size(12.0).color(color)).truncate());
                if let Some(reason) = self.shared.auto_paused.get() {
                    label.on_hover_text(reason);
                }
            });
        }
        if toggle {
            self.settings.appearance.compact_sidebar = !self.settings.appearance.compact_sidebar;
            self.save_settings();
        }
        ui.add_space(6.0);
        let pages_height = (ui.available_height() - widgets::PILL_HEIGHT - 10.0).max(36.0);
        egui::ScrollArea::vertical()
            .id_salt("pages")
            .max_height(pages_height)
            .auto_shrink([false, true])
            .drag_to_scroll(false)
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                for page in Page::ALL {
                    let current = self.page == page;
                    if widgets::nav_row(ui, page.icon(), page.label(), current, rail).clicked() {
                        self.save_typed();
                        self.page = page;
                        self.recording = None;
                    }
                }
            });
        ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
            let any = !self.store.rules.is_empty();
            if self.store.paused {
                let resume = if rail {
                    widgets::round_icon(ui, Icon::PLAY, "Resume")
                } else {
                    widgets::sized(ui, "Resume", true, ui.available_width())
                };
                if resume.clicked() {
                    self.set_paused(false);
                }
            } else {
                let pause = ui
                    .add_enabled_ui(any, |ui| {
                        if rail {
                            widgets::round_icon(ui, Icon::PAUSE, "Pause all")
                        } else {
                            widgets::sized(ui, "Pause all", false, ui.available_width())
                                .on_hover_text("Put every app back without deleting rules.")
                        }
                    })
                    .inner;
                if pause.clicked() {
                    self.set_paused(true);
                }
            }
        });
    }

    /// What Blurman is doing right now, in a few words.
    fn status_line(&self) -> (String, egui::Color32) {
        let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
        let c = colors();
        if self.store.paused {
            ("Paused".to_string(), c.warn)
        } else if self.shared.auto_paused.get().is_some() {
            ("Auto-paused".to_string(), c.warn)
        } else if frosted == 0 {
            ("Nothing frosted yet".to_string(), c.muted)
        } else if frosted == 1 {
            ("Frosting 1 app".to_string(), c.muted)
        } else {
            (format!("Frosting {frosted} apps"), c.muted)
        }
    }

    /// Notes that apply to every page: an update or a problem.
    fn banners(&mut self, ui: &mut Ui) {
        let c = colors();
        if let Some(update) = self.shared.update.get() {
            widgets::banner(
                ui,
                |ui| {
                    if widgets::primary(ui, "Download").clicked() {
                        system::open(&update.url);
                    }
                    if widgets::secondary(ui, "Not now").clicked() {
                        self.shared.set_update(None);
                    }
                },
                |ui| {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("Blurman {} is out.", update.version))
                                .color(c.text),
                        )
                        .wrap(),
                    );
                },
            );
        }
        let status = self.shared.status();
        if !status.is_empty() {
            widgets::banner(
                ui,
                |ui| {
                    if widgets::secondary(ui, "Dismiss").clicked() {
                        self.shared.set_status("");
                    }
                },
                |ui| {
                    ui.label(
                        RichText::new("Something went wrong")
                            .font(theme::semibold(13.0))
                            .color(c.text),
                    );
                    ui.add(egui::Label::new(theme::muted(status)).wrap());
                },
            );
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
        // Between the islands there is only the desktop, which is not Blurman's to dim.
        let dim = if theme::composed() || colors().high_contrast {
            0
        } else {
            100
        };
        let width = (ctx.screen_rect().width() - 96.0).clamp(200.0, 380.0);
        let modal = egui::Modal::new(egui::Id::new("confirm"))
            .frame(theme::dialog())
            .backdrop_color(egui::Color32::from_black_alpha(dim))
            .show(ctx, |ui| {
                ui.set_width(width);
                ui.add(
                    egui::Label::new(
                        RichText::new(title)
                            .font(theme::semibold(17.0))
                            .color(colors().text),
                    )
                    .wrap(),
                );
                ui.add_space(2.0);
                ui.add(egui::Label::new(theme::muted(body)).wrap());
                ui.add_space(12.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::danger(ui, action).clicked() {
                        answer = Some(true);
                    }
                    let cancel = widgets::secondary(ui, "Cancel");
                    if cancel.clicked() {
                        answer = Some(false);
                    }
                    // The keyboard starts on the answer that changes nothing.
                    if ui.memory(|memory| memory.focused().is_none()) {
                        cancel.request_focus();
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

/// A settings-like page: islands that scroll under the header, their edges in line with it.
fn column(ui: &mut Ui, id: &str, add: impl FnOnce(&mut Ui)) {
    let width = ui.available_width();
    egui::ScrollArea::vertical()
        .id_salt(id)
        .auto_shrink([false, false])
        .drag_to_scroll(false)
        .show(ui, |ui| {
            ui.set_width(width);
            add(ui);
        });
}

impl eframe::App for BlurmanApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        match self.backdrop.mode {
            Mode::Glass | Mode::Frosted | Mode::Islands => [0.0, 0.0, 0.0, 0.0],
            Mode::Solid | Mode::HighContrast => colors().bg.to_normalized_gamma_f32(),
        }
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.follow_look(ctx);
        if ctx.input(|input| input.viewport().close_requested()) {
            self.save_typed();
            // A drag can end by closing the window, before the settle timer writes the file.
            if self.pending_save.is_some() {
                self.flush_rules_file();
            }
            if self.keeps_in_tray()
                && !self.shared.exiting.load(Ordering::SeqCst)
                && !REOPEN.load(Ordering::SeqCst)
            {
                self.shared.to_tray.store(true, Ordering::SeqCst);
            }
        }
        self.sync_from_disk();
        self.sync_settings_from_disk();
        if let Some(process) = self.shared.picked.take() {
            self.rescan();
            self.select(process);
        }
        self.record_shortcut(ctx);
        if self.scanned.elapsed() >= RESCAN {
            self.rescan();
            self.remake_if_needed(ctx);
        }
        if matches!(self.backdrop.mode, Mode::Glass | Mode::Islands) {
            // Such a shell styles the frame again when the window in front changes, a moment
            // after the change: ask again for a second after each one, and once a second besides.
            let focused = ctx.input(|input| input.focused);
            let watch = egui::Id::new("keep-bare");
            let (was, until) = ctx
                .data(|data| data.get_temp::<(bool, Instant)>(watch))
                .unwrap_or((!focused, Instant::now()));
            let until = if was == focused {
                until
            } else {
                Instant::now() + Duration::from_secs(1)
            };
            ctx.data_mut(|data| data.insert_temp(watch, (focused, until)));
            let settling = Instant::now() < until;
            if settling || self.scanned.elapsed() < Duration::from_millis(50) {
                keep_bare(target::hwnd_of(self.shared.main_window.load(Ordering::SeqCst)));
            }
            if settling {
                ctx.request_repaint_after(Duration::from_millis(60));
            }
        }
        if self.wants_tray() {
            self.tray_error = tray::sync(true, &self.store).err();
        }
        self.match_dpi(ctx);
        // A saved position or size can leave the window off screen. Fix it as it opens.
        if self.opened.elapsed() < PLACE_FOR {
            let hwnd = target::hwnd_of(self.shared.main_window.load(Ordering::SeqCst));
            if !hwnd.0.is_null() {
                keep_on_screen(hwnd);
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(theme::page_fill()))
            .show(ctx, |ui| self.shell(ui));
        self.confirm_dialog(ctx);
        let toast = self.draw_toast(ctx);
        self.share_recording();
        self.resize_edges(ctx);
        let islands = widgets::take_islands();
        if let Some(panel) = &self.backdrop.panel {
            panel.set_islands(&islands, ctx.pixels_per_point());
        }
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
        let grown = SetWindowPos(
            hwnd,
            None,
            0,
            0,
            wanted_w + chrome_w,
            wanted_h + chrome_h,
            SWP_NOMOVE | SWP_NOZORDER | SWP_NOACTIVATE,
        )
        .is_ok();
        // Growing must never push the window past its monitor.
        keep_on_screen(hwnd);
        grown
    }
}

/// Bring the window fully onto the monitor it is nearest to: no larger than that monitor's work
/// area, and with no edge hanging off it. A window saved on a monitor that is gone, or saved
/// larger than the screen, comes back whole.
fn keep_on_screen(hwnd: HWND) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowRect, IsIconic, IsZoomed, SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };

    unsafe {
        if IsIconic(hwnd).as_bool() || IsZoomed(hwnd).as_bool() {
            return;
        }
        let mut window = RECT::default();
        if GetWindowRect(hwnd, &mut window).is_err() {
            return;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let fitted = fit_frame_in(window, target::frame_bounds(hwnd), info.rcWork);
        if fitted != window {
            let _ = SetWindowPos(
                hwnd,
                None,
                fitted.left,
                fitted.top,
                fitted.right - fitted.left,
                fitted.bottom - fitted.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
    }
}

/// `fit_in` by the frame you can see. Windows counts the invisible resize borders as part of the
/// window, so one snapped to a screen edge reaches a few pixels past the work area with nothing
/// showing there. Fitted by its own rectangle it would count as larger than the screen and be
/// shrunk each time it opens.
fn fit_frame_in(
    window: windows::Win32::Foundation::RECT,
    frame: windows::Win32::Foundation::RECT,
    work: windows::Win32::Foundation::RECT,
) -> windows::Win32::Foundation::RECT {
    let borders = [
        frame.left - window.left,
        frame.top - window.top,
        window.right - frame.right,
        window.bottom - frame.bottom,
    ];
    // Borders are a few pixels. Anything else is not a frame to trust.
    if borders.iter().any(|border| !(0..=64).contains(border)) {
        return fit_in(window, work);
    }
    let fitted = fit_in(frame, work);
    windows::Win32::Foundation::RECT {
        left: fitted.left - borders[0],
        top: fitted.top - borders[1],
        right: fitted.right + borders[2],
        bottom: fitted.bottom + borders[3],
    }
}

/// `window` moved and shrunk as little as possible to lie inside `work`. Anything larger than
/// the work area is brought down to most of it, so the window still looks like a window.
fn fit_in(
    window: windows::Win32::Foundation::RECT,
    work: windows::Win32::Foundation::RECT,
) -> windows::Win32::Foundation::RECT {
    let work_w = work.right - work.left;
    let work_h = work.bottom - work.top;
    let mut width = window.right - window.left;
    let mut height = window.bottom - window.top;
    if width > work_w {
        width = work_w * 9 / 10;
    }
    if height > work_h {
        height = work_h * 9 / 10;
    }
    let left = window.left.clamp(work.left, work.right - width);
    let top = window.top.clamp(work.top, work.bottom - height);
    windows::Win32::Foundation::RECT {
        left,
        top,
        right: left + width,
        bottom: top + height,
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
