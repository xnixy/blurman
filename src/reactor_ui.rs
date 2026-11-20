//! Spike: Blurman's window as a native WinUI 3 app, through Microsoft's windows-reactor, drawn
//! after the "Blurman for Windows" design boards: one Mica surface, a sidebar with what Blurman
//! is doing at its top, Windows Settings-style rows in cards, and WinUI's type ramp.
//!
//! Run with `BLURMAN_UI=reactor`. It drives the same effect thread as the egui window: rules and
//! settings are saved and the effect thread is signalled the same way, and slider drags go out as
//! live tweaks. Apps and Settings are built; Presets, Shortcuts and Auto-pause are placeholders.

use crate::ipc;
use crate::mapping::{self, BlurStyle, FocusLook, Look, TintSource};
use crate::rules::{self, Rule, Settings, Store};
use crate::shared::{Shared, Tweak};
use crate::target::{self, AppGroup};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, SetWindowsHookExW, MSG, PM_REMOVE, WH_GETMESSAGE,
};
use windows_reactor::*;

/// The new icon (assets/icon/*.svg, rendered by assets/icon/render.ps1).
/// The title bar shows it at 16 px, where the small variant (no window bar) reads better.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon-small-32.png");
const APP_ICO: &[u8] = include_bytes!("../assets/blurman.ico");
const TRAY_ICO: &[u8] = include_bytes!("../assets/tray.ico");
const TRAY_PAUSED_ICO: &[u8] = include_bytes!("../assets/tray-paused.ico");
/// How often the app list is read again, as the egui window does.
const RESCAN: Duration = Duration::from_secs(1);
/// How long a slider rests before its rule is written to disk.
const SAVE_AFTER: Duration = Duration::from_millis(600);
/// WinUI's display face, for page titles and the selected app's name.
const DISPLAY_FONT: &str = "Segoe UI Variable Display";

thread_local! {
    /// Where the UI thread's "open the window" message goes: the application, while it runs.
    static OPEN: RefCell<Option<LocalSender<AppMessage>>> = const { RefCell::new(None) };
}

/// A second launch, the tray or a shortcut asks for the window with a thread message
/// (`tray::WM_OPEN`, from `Shared::show_window`). Reactor's message loop has no handler for thread
/// messages, so this hook sees them as they are taken off the queue and tells the application.
unsafe extern "system" fn watch_thread_messages(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == PM_REMOVE.0 as usize {
        let message = &*(lparam.0 as *const MSG);
        if message.hwnd.0.is_null() && message.message == crate::tray::WM_OPEN {
            OPEN.with(|open| {
                if let Some(sender) = open.borrow().as_ref() {
                    let _ = sender.send(AppMessage::Open);
                }
            });
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// The icon files Windows wants by path (the window's and the tray's), written once beside the
/// other temporary files of this profile.
struct IconFiles {
    app: String,
    tray: String,
    paused: String,
}

fn icon_files() -> &'static IconFiles {
    static FILES: OnceLock<IconFiles> = OnceLock::new();
    FILES.get_or_init(|| {
        let write = |name: &str, bytes: &[u8]| {
            let path = std::env::temp_dir().join(format!("{}-{name}", ipc::scoped("blurman")));
            if std::fs::read(&path).ok().as_deref() != Some(bytes) {
                let _ = std::fs::write(&path, bytes);
            }
            path.to_string_lossy().into_owned()
        };
        IconFiles {
            app: write("app.ico", APP_ICO),
            tray: write("tray.ico", TRAY_ICO),
            paused: write("tray-paused.ico", TRAY_PAUSED_ICO),
        }
    })
}

/// The shared state, compared by identity so it can be a window's input.
#[derive(Clone)]
struct Handle(Arc<Shared>);

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Run the WinUI window and its tray icon until Exit. With `startup`, start in the tray.
pub fn run(shared: Arc<Shared>, startup: bool) -> Result<(), String> {
    shared
        .ui_thread
        .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    App::run_application::<Blurman>((Handle(shared), startup)).map_err(|err| err.to_string())
}

// ---------------------------------------------------------------------------------------------
// The application: one reopenable window, and the tray icon while Blurman keeps to the tray.

struct Blurman {
    paused: bool,
    tray: bool,
}

#[derive(Clone)]
enum AppMessage {
    Open,
    Menu(Key),
    /// The window changed the rules or settings: look again at what the tray shows.
    Changed,
}

#[derive(Clone, PartialEq)]
struct WindowInput {
    shared: Handle,
    changed: Callback<()>,
}

impl Blurman {
    fn read(startup: bool) -> Self {
        Self {
            paused: rules::load().paused,
            tray: startup || rules::load_settings().close_to_tray,
        }
    }
}

impl Application for Blurman {
    type Input = (Handle, bool);
    type Message = AppMessage;

    fn create(input: &(Handle, bool), context: &ApplicationContext<Self>) -> Self {
        OPEN.with(|open| *open.borrow_mut() = Some(context.sender()));
        unsafe {
            let _ = SetWindowsHookExW(
                WH_GETMESSAGE,
                Some(watch_thread_messages),
                None,
                GetCurrentThreadId(),
            );
        }
        if !input.1 {
            context.show_window("main");
        }
        Self::read(input.1)
    }

    fn update(&mut self, message: AppMessage, context: &ApplicationContext<Self>) {
        match message {
            AppMessage::Open => context.show_window("main"),
            AppMessage::Menu(key) => match key.as_str() {
                Some("open") => context.show_window("main"),
                Some("pause") => {
                    if let Ok(mut store) = rules::read() {
                        store.paused = !store.paused;
                        if rules::save(&store).is_ok() {
                            ipc::signal(ipc::msg_reload());
                        }
                        self.paused = store.paused;
                    }
                }
                Some("exit") => context.exit(),
                _ => {}
            },
            AppMessage::Changed => {
                let tray = self.tray;
                *self = Self::read(false);
                self.tray |= tray;
            }
        }
    }

    fn view(&self, input: &(Handle, bool), context: &ApplicationViewContext<Self>) -> ApplicationView {
        let view = ApplicationView::new().window::<MainWindow>(
            "main",
            WindowInput {
                shared: input.0.clone(),
                changed: context.message(AppMessage::Changed),
            },
        );
        if !self.tray {
            return view;
        }
        let files = icon_files();
        view.notify_icon(
            "tray",
            NotifyIcon::new(if self.paused { &files.paused } else { &files.tray })
                .tooltip(if self.paused { "Blurman: paused" } else { "Blurman" })
                .on_activate(context.callback(|_| AppMessage::Open))
                .menu(Menu::new(
                    [
                        MenuItem::item("open", "Open Blurman"),
                        MenuItem::item("pause", if self.paused { "Resume" } else { "Pause all" }),
                        MenuItem::separator("separator"),
                        MenuItem::item("exit", "Exit"),
                    ],
                    context.callback(AppMessage::Menu),
                )),
        )
    }
}

// ---------------------------------------------------------------------------------------------
// The window.

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Apps,
    Presets,
    Shortcuts,
    AutoPause,
    Settings,
}

#[derive(Clone, Copy, PartialEq)]
enum Tab {
    Look,
    Finish,
    Behavior,
}

/// One row of the app list: from a rule, an open app, or both.
struct Entry {
    process: String,
    subtitle: String,
    path: Option<String>,
    /// Some(on) when the app has a rule.
    enabled: Option<bool>,
    elevated: bool,
}

struct MainWindow {
    shared: Arc<Shared>,
    store: Store,
    settings: Settings,
    startup: bool,
    groups: Vec<AppGroup>,
    /// PNG of each exe's icon at 48 px, by path; None when it has none.
    icons: HashMap<String, Option<EncodedImage>>,
    app_icon: EncodedImage,
    search: String,
    selected: Option<String>,
    page: Page,
    tab: Tab,
    seen_generation: u64,
    seen_settings: u64,
    /// A slider moved and its rule is not on disk yet.
    unsaved: bool,
    asking_delete: bool,
    asking_delete_all: bool,
    only_draft: String,
    skip_draft: String,
    recording: bool,
    recorded: Option<String>,
    hwnd: isize,
    /// The element the glass preview's visual sits in, and the preview once WinUI has it.
    preview_host: ElementRef<Grid>,
    preview: Option<crate::preview::Preview>,
    _tick: Option<ComponentTimer>,
    _save: Option<ComponentTimer>,
}

#[derive(Clone)]
enum Message {
    Tick,
    Hwnd(isize),
    Page(Page),
    Tab(Option<Rc<str>>),
    Search(Rc<str>),
    /// A row picked in one of the list's three groups.
    Select(usize, Option<usize>),
    Enabled(String, bool),
    Frost(String),
    Style(Option<usize>),
    Transparency(f64),
    Blur(f64),
    Preset(Option<usize>),
    TintSource(Option<usize>),
    TintAmount(f64),
    Saturation(f64),
    Milk(f64),
    Contrast(f64),
    Grain(f64),
    Focus(Option<usize>),
    OnlyDraft(Rc<str>),
    AddOnly,
    RemoveOnly(usize),
    SkipDraft(Rc<str>),
    AddSkip,
    RemoveSkip(usize),
    Save,
    AskDelete,
    DeleteClosed(ContentDialogResult),
    TogglePause,
    Theme(Option<usize>),
    Glass(Option<usize>),
    Accent(String),
    Compact(bool),
    Startup(bool),
    Tray(bool),
    Updates(bool),
    OpenFolder,
    AskDeleteAll,
    DeleteAllClosed(ContentDialogResult),
    Record,
    /// A recorded shortcut, or None when Esc cancelled.
    Recorded(Option<String>),
    /// WinUI has the preview's element ready, or it changed size.
    PreviewHost(CompositionHostEvent),
}

const THEMES: [&str; 3] = ["windows", "light", "dark"];
const GLASSES: [&str; 3] = ["mica", "mica-alt", "acrylic"];
const TINTS: [TintSource; 3] = [TintSource::Custom, TintSource::Theme, TintSource::Accent];
const FOCUSES: [FocusLook; 3] = [FocusLook::Same, FocusLook::Lighter, FocusLook::Solid];

impl Component for MainWindow {
    type Input = WindowInput;
    type Message = Message;

    fn create(input: &WindowInput, context: &ComponentContext<Self>) -> Self {
        // The window handle, for Win32 work on the window (bringing it forward from a second
        // launch, keeping it on screen). Reactor hands it out on the UI thread.
        let _ = context.run_window(|window| Message::Hwnd(window.as_raw() as isize));
        let shared = input.shared.0.clone();
        let mut window = Self {
            seen_generation: shared.rules_generation.load(Ordering::SeqCst),
            seen_settings: shared.settings_generation.load(Ordering::SeqCst),
            shared,
            store: rules::load(),
            settings: rules::load_settings(),
            startup: crate::autostart::is_enabled(),
            groups: Vec::new(),
            icons: HashMap::new(),
            app_icon: EncodedImage::new(ICON_PNG.to_vec()),
            search: String::new(),
            selected: None,
            page: Page::Apps,
            tab: Tab::Look,
            unsaved: false,
            asking_delete: false,
            asking_delete_all: false,
            only_draft: String::new(),
            skip_draft: String::new(),
            recording: false,
            recorded: None,
            hwnd: 0,
            preview_host: ElementRef::new(),
            preview: None,
            _tick: None,
            _save: None,
        };
        window.rescan();
        window._tick = Some(context.set_local_timeout(RESCAN, || Message::Tick));
        window
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Tick => {
                self.rescan();
                self.sync_from_disk();
                self._tick = Some(context.set_local_timeout(RESCAN, || Message::Tick));
            }
            Message::Hwnd(hwnd) => {
                self.hwnd = hwnd;
                self.shared.main_window.store(hwnd, Ordering::SeqCst);
            }
            Message::Page(page) => self.page = page,
            Message::Tab(text) => {
                self.tab = match text.as_deref() {
                    Some("Finish") => Tab::Finish,
                    Some("Behavior") => Tab::Behavior,
                    _ => Tab::Look,
                }
            }
            Message::Search(text) => self.search = text.to_string(),
            Message::Select(group, Some(index)) => {
                if let Some(entry) = self.entries().into_iter().nth(group).and_then(|g| g.into_iter().nth(index)) {
                    self.selected = Some(entry.process);
                }
            }
            // A list rebuilt under a search, or another group's pick, reports no selection here;
            // keep the app being edited.
            Message::Select(_, None) => {}
            Message::Enabled(process, on) => {
                if let Some(rule) = self.store.rule_mut(&process) {
                    if rule.enabled != on {
                        rule.enabled = on;
                        self.publish(context);
                    }
                }
            }
            Message::Frost(process) => {
                self.store.toggle_app(&process, Look::default());
                self.selected = Some(process);
                self.tab = Tab::Look;
                self.publish(context);
            }
            Message::Style(Some(index)) => {
                let style = if index == 1 { BlurStyle::Acrylic } else { BlurStyle::Frost };
                self.change_look(context, |look| look.style = style);
            }
            Message::Transparency(value) => {
                let value = clamp(value, mapping::TRANSPARENCY_MIN, mapping::TRANSPARENCY_MAX);
                self.change_look(context, |look| look.transparency = value);
            }
            Message::Blur(value) => {
                let value = clamp(value, mapping::BLUR_MIN, mapping::BLUR_MAX);
                self.change_look(context, |look| look.blur = value);
            }
            Message::Preset(Some(index)) => {
                if let Some(preset) = self.settings.presets.get(index) {
                    let look = preset.look;
                    self.change_look(context, |current| *current = look);
                }
            }
            Message::TintSource(Some(index)) => {
                self.change_look(context, |look| look.finish.tint_source = TINTS[index.min(2)]);
            }
            Message::TintAmount(value) => {
                let value = clamp(value, 0, mapping::TINT_MAX);
                self.change_look(context, |look| look.finish.tint_amount = value);
            }
            Message::Saturation(value) => {
                let value = clamp(value, mapping::SATURATION_MIN, mapping::SATURATION_MAX);
                self.change_look(context, |look| look.finish.saturation = value);
            }
            Message::Milk(value) => {
                let value = clamp(value, 0, mapping::MILK_MAX);
                self.change_look(context, |look| look.finish.milk = value);
            }
            Message::Contrast(value) => {
                let value = clamp(value, 0, mapping::CONTRAST_MAX);
                self.change_look(context, |look| look.finish.contrast = value);
            }
            Message::Grain(value) => {
                let value = clamp(value, 0, mapping::GRAIN_MAX);
                self.change_look(context, |look| look.finish.grain = value);
            }
            Message::Focus(Some(index)) => {
                self.edit_rule(context, |rule| rule.focus = FOCUSES[index.min(2)]);
            }
            Message::Style(None)
            | Message::Preset(None)
            | Message::TintSource(None)
            | Message::Focus(None)
            | Message::Theme(None)
            | Message::Glass(None) => {}
            Message::OnlyDraft(text) => self.only_draft = text.to_string(),
            Message::AddOnly => {
                let title = std::mem::take(&mut self.only_draft).trim().to_string();
                if !title.is_empty() {
                    self.edit_rule(context, |rule| rule.only_titles.push(title));
                }
            }
            Message::RemoveOnly(index) => {
                self.edit_rule(context, |rule| {
                    if index < rule.only_titles.len() {
                        rule.only_titles.remove(index);
                    }
                });
            }
            Message::SkipDraft(text) => self.skip_draft = text.to_string(),
            Message::AddSkip => {
                let title = std::mem::take(&mut self.skip_draft).trim().to_string();
                if !title.is_empty() {
                    self.edit_rule(context, |rule| rule.skip_titles.push(title));
                }
            }
            Message::RemoveSkip(index) => {
                self.edit_rule(context, |rule| {
                    if index < rule.skip_titles.len() {
                        rule.skip_titles.remove(index);
                    }
                });
            }
            Message::Save => {
                if self.unsaved {
                    self.publish(context);
                }
            }
            Message::AskDelete => self.asking_delete = true,
            Message::DeleteClosed(result) => {
                self.asking_delete = false;
                if matches!(result, ContentDialogResult::Primary) {
                    if let Some(process) = self.selected.clone() {
                        self.store
                            .rules
                            .retain(|rule| !rule.process.eq_ignore_ascii_case(&process));
                        self.publish(context);
                    }
                }
            }
            Message::TogglePause => {
                self.store.paused = !self.store.paused;
                self.publish(context);
            }
            Message::Theme(Some(index)) => {
                self.settings.appearance.theme = THEMES[index.min(2)].into();
                self.save_settings(context);
            }
            Message::Glass(Some(index)) => {
                self.settings.appearance.window = GLASSES[index.min(2)].into();
                self.save_settings(context);
            }
            Message::Accent(name) => {
                self.settings.appearance.accent = name;
                self.save_settings(context);
            }
            Message::Compact(on) => {
                self.settings.appearance.compact_sidebar = on;
                self.save_settings(context);
            }
            Message::Startup(on) => {
                if let Err(err) = crate::autostart::set_enabled(on) {
                    self.shared.set_status(err);
                }
                self.startup = crate::autostart::is_enabled();
            }
            Message::Tray(on) => {
                self.settings.close_to_tray = on;
                self.save_settings(context);
            }
            Message::Updates(on) => {
                self.settings.check_updates = on;
                self.save_settings(context);
            }
            Message::OpenFolder => {
                if let Some(folder) = rules::rules_path().parent() {
                    let _ = std::process::Command::new("explorer").arg(folder).spawn();
                }
            }
            Message::AskDeleteAll => self.asking_delete_all = true,
            Message::DeleteAllClosed(result) => {
                self.asking_delete_all = false;
                if matches!(result, ContentDialogResult::Primary) {
                    self.store.rules.clear();
                    self.publish(context);
                }
            }
            Message::Record => {
                self.recording = true;
                self.recorded = None;
            }
            Message::Recorded(text) => {
                self.recording = false;
                self.recorded = text;
            }
            Message::PreviewHost(CompositionHostEvent::Ready {
                compositor,
                width,
                height,
                ..
            }) => match crate::preview::Preview::new(compositor) {
                Ok(preview) => {
                    preview.resize(width, height);
                    let _ = self
                        .preview_host
                        .request_set_child_visual(Some(preview.visual()), |_| {});
                    self.preview = Some(preview);
                }
                Err(err) => eprintln!("The glass preview is unavailable ({err})."),
            },
            Message::PreviewHost(CompositionHostEvent::Metrics { width, height, .. }) => {
                if let Some(preview) = &self.preview {
                    preview.resize(width, height);
                }
            }
        }
        self.refresh_preview();
    }

    fn view(&self, _input: &WindowInput, context: &mut ViewContext<Self>) -> View {
        context.window_title("Blurman");
        // The glass preview: watch its element while the editor shows one.
        let showing = self.showing_preview();
        let host = self.preview_host.clone();
        let sender = context.sender();
        context.use_effect_guard("glass-preview", showing, move || {
            showing.then(|| {
                host.observe_composition_host(move |event| {
                    let _ = sender.send(Message::PreviewHost(event));
                })
            })
        });
        context.window_visuals(
            WindowVisuals::new()
                .client_size(1180.0, 780.0)
                .backdrop(self.backdrop())
                .theme(self.theme())
                .icon(icon_files().app.as_str()),
        );
        let page: View = match self.page {
            Page::Apps => self.apps_page(context),
            Page::Settings => self.settings_page(context),
            Page::Shortcuts => self.shortcuts_page(context),
            Page::Presets => placeholder_page("Presets", "Saved looks you can put on any app. Not part of this spike."),
            Page::AutoPause => placeholder_page("Auto-pause", "Pausing on its own: battery saver, Do not disturb, presentations, set hours. Not part of this spike."),
        };
        let title_bar = TitleBar::new()
            .title("Blurman")
            .icon(Icon::image_data(self.app_icon.clone()))
            .grid_row(0);
        let body = Grid::new()
            .columns([GridLength::Auto, GridLength::STAR])
            .grid_row(1)
            .children((
                self.sidebar(context),
                Border::new()
                    .grid_column(1)
                    .content(Grid::new().keyed_children([keyed(page_key(self.page), page)])),
            ));
        let root = Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title_bar, body));

        // The shortcut recorder: while recording, every key press goes to it first.
        let recording = self.recording;
        let recorded = context.callback(Message::Recorded);
        Border::new()
            .on_preview_key_down(RoutedCallback::new(move |info: KeyEventInfo| {
                if !recording {
                    return false;
                }
                if info.key == VirtualKey::ESCAPE {
                    recorded.call(None);
                } else if let Some(text) = shortcut_text(&info) {
                    recorded.call(Some(text));
                }
                true
            }))
            .content(root)
            .into()
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        // The window is closing to the tray: there is no window to bring forward any more.
        let _ = self.shared.main_window.compare_exchange(
            self.hwnd,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        if self.unsaved {
            self.store.normalize();
            if rules::save(&self.store).is_ok() {
                ipc::signal(ipc::msg_reload());
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// State.

impl MainWindow {
    fn rescan(&mut self) {
        self.groups = target::list_groups();
        for group in &self.groups {
            if let Some(path) = &group.path {
                if !self.icons.contains_key(path) {
                    self.icons.insert(path.clone(), icon_png(path));
                }
            }
        }
    }

    /// Pick up rule and settings changes made from the tray, a shortcut or the command line.
    fn sync_from_disk(&mut self) {
        let generation = self.shared.rules_generation.load(Ordering::SeqCst);
        if generation != self.seen_generation && !self.unsaved {
            if let Ok(store) = rules::read() {
                self.store = store;
                self.seen_generation = generation;
            }
        }
        let settings = self.shared.settings_generation.load(Ordering::SeqCst);
        if settings != self.seen_settings {
            self.settings = rules::load_settings();
            self.seen_settings = settings;
        }
    }

    /// Change the selected app's look, show it on its windows now, and save once it rests.
    fn change_look(&mut self, context: &ComponentContext<Self>, change: impl FnOnce(&mut Look)) {
        let Some(process) = self.selected.clone() else {
            return;
        };
        let Some(rule) = self.store.rule_mut(&process) else {
            return;
        };
        let before = rule.look;
        change(&mut rule.look);
        if rule.look == before {
            return;
        }
        self.shared.push_tweak(Tweak {
            process: rule.process.clone(),
            look: rule.look,
        });
        ipc::signal(ipc::msg_tweak());
        self.unsaved = true;
        self._save = Some(context.set_local_timeout(SAVE_AFTER, || Message::Save));
    }

    /// Change something of the selected app's rule other than its look, and save it now.
    fn edit_rule(&mut self, context: &ComponentContext<Self>, change: impl FnOnce(&mut Rule)) {
        let Some(process) = self.selected.clone() else {
            return;
        };
        if let Some(rule) = self.store.rule_mut(&process) {
            change(rule);
            self.publish(context);
        }
    }

    /// Write the rules, have the effect thread reload them, and let the tray know.
    fn publish(&mut self, context: &ComponentContext<Self>) {
        self.unsaved = false;
        self.store.normalize();
        if let Err(err) = rules::save(&self.store) {
            self.shared.set_status(format!("Could not save rules: {err}"));
            return;
        }
        ipc::signal(ipc::msg_reload());
        context.input().changed.call(());
    }

    fn save_settings(&mut self, context: &ComponentContext<Self>) {
        match rules::save_settings(&self.settings) {
            Ok(()) => {
                ipc::signal(ipc::msg_settings());
                context.input().changed.call(());
            }
            Err(err) => self.shared.set_status(format!("Could not save settings: {err}")),
        }
    }

    fn selected_rule(&self) -> Option<&Rule> {
        self.selected
            .as_ref()
            .and_then(|process| self.store.rule(process))
    }

    /// Whether the editor shows the glass preview: an app with a rule, on the Apps page.
    fn showing_preview(&self) -> bool {
        self.page == Page::Apps && self.selected_rule().is_some()
    }

    /// Keep the preview showing the selected app's look; let it go when it is not shown.
    fn refresh_preview(&mut self) {
        if !self.showing_preview() {
            self.preview = None;
            return;
        }
        if let (Some(preview), Some(rule)) = (&self.preview, self.selected_rule()) {
            let _ = preview.set_look(&rule.look);
        }
    }

    /// Whether the window is dark: its theme setting, or Windows' app mode.
    fn dark(&self) -> bool {
        match self.settings.appearance.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => !crate::theme::apps_light(),
        }
    }

    fn theme(&self) -> WindowTheme {
        match self.settings.appearance.theme.as_str() {
            "light" => WindowTheme::Light,
            "dark" => WindowTheme::Dark,
            _ => WindowTheme::System,
        }
    }

    fn backdrop(&self) -> WindowBackdrop {
        match self.settings.appearance.window.as_str() {
            "mica-alt" => WindowBackdrop::MicaAlt,
            "acrylic" => WindowBackdrop::Acrylic,
            _ => WindowBackdrop::Mica,
        }
    }

    fn group(&self, process: &str) -> Option<&AppGroup> {
        self.groups
            .iter()
            .find(|group| group.process.eq_ignore_ascii_case(process))
    }

    fn icon_of(&self, path: Option<&String>) -> Option<EncodedImage> {
        path.and_then(|path| self.icons.get(path)).cloned().flatten()
    }

    /// Frosted apps, apps whose rule is off, and everything else that is open, each matching the
    /// search.
    fn entries(&self) -> [Vec<Entry>; 3] {
        let needle = self.search.trim().to_lowercase();
        let matches = |process: &str, title: &str| {
            needle.is_empty()
                || process.to_lowercase().contains(&needle)
                || title.to_lowercase().contains(&needle)
        };
        let mut lists: [Vec<Entry>; 3] = Default::default();
        for rule in &self.store.rules {
            let group = self.group(&rule.process);
            let title = group.map(|group| group.sample_title.as_str()).unwrap_or("");
            if !matches(&rule.process, title) {
                continue;
            }
            lists[if rule.enabled { 0 } else { 1 }].push(Entry {
                process: rule.process.clone(),
                subtitle: subtitle(group),
                path: group.and_then(|group| group.path.clone()),
                enabled: Some(rule.enabled),
                elevated: group.is_some_and(|group| group.elevated),
            });
        }
        for group in &self.groups {
            if self.store.rule(&group.process).is_some()
                || !matches(&group.process, &group.sample_title)
            {
                continue;
            }
            lists[2].push(Entry {
                process: group.process.clone(),
                subtitle: subtitle(Some(group)),
                path: group.path.clone(),
                enabled: None,
                elevated: group.elevated,
            });
        }
        lists
    }
}

// ---------------------------------------------------------------------------------------------
// The sidebar.

impl MainWindow {
    fn sidebar(&self, context: &ViewContext<Self>) -> View {
        let compact = self.settings.appearance.compact_sidebar;
        let mut top: Vec<View> = Vec::new();
        if compact {
            let label = if self.store.paused { "Resume" } else { "Pause all" };
            top.push(
                Button::new()
                    .style(ButtonStyle::Subtle)
                    .height(40.0)
                    .horizontal_alignment(HorizontalAlignment::Stretch)
                    .margin(Thickness::new(0.0, 4.0, 0.0, 10.0))
                    .automation_name(label)
                    .on_click(context.message(Message::TogglePause))
                    .content(glyph(if self.store.paused { "\u{E768}" } else { "\u{E769}" }, 16.0))
                    .into(),
            );
        } else {
            top.push(self.status_card(context));
        }
        top.push(self.nav(context, Page::Apps, "Apps", "\u{E71D}", compact));
        top.push(self.nav(context, Page::Presets, "Presets", "\u{E734}", compact));
        top.push(self.nav(context, Page::Shortcuts, "Shortcuts", "\u{E765}", compact));
        top.push(self.nav(context, Page::AutoPause, "Auto-pause", "\u{E708}", compact));
        Grid::new()
            .width(if compact { 64.0 } else { 248.0 })
            .margin(Thickness::new(12.0, 0.0, 12.0, 16.0))
            .rows([GridLength::Auto, GridLength::STAR, GridLength::Auto])
            .children((
                StackPanel::new().spacing(2.0).children(top),
                Border::new()
                    .grid_row(2)
                    .content(self.nav(context, Page::Settings, "Settings", "\u{E713}", compact)),
            ))
            .into()
    }

    /// What Blurman is doing right now, with Pause all or Resume.
    fn status_card(&self, context: &ViewContext<Self>) -> View {
        let paused = self.store.paused;
        let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
        let dot: View = if paused {
            Ellipse::new()
                .width(8.0)
                .height(8.0)
                .stroke(ThemeBrush::SecondaryText)
                .stroke_thickness(1.5)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        } else {
            Ellipse::new()
                .width(8.0)
                .height(8.0)
                .fill(ThemeBrush::Accent)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        };
        let heading = if paused {
            "Paused".to_string()
        } else if frosted == 1 {
            "Frosting 1 app".to_string()
        } else {
            format!("Frosting {frosted} apps")
        };
        let mut button = Button::new()
            .on_click(context.message(Message::TogglePause))
            .content(with_glyph(
                if paused { "\u{E768}" } else { "\u{E769}" },
                if paused { "Resume" } else { "Pause all" },
            ));
        if paused {
            button = button.style(ButtonStyle::Accent);
        }
        card(
            Thickness::new(14.0, 12.0, 14.0, 12.0),
            StackPanel::new().spacing(10.0).children((
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .children((dot, strong(&heading))),
                caption(if paused {
                    "Every app is back to normal. Your rules are kept."
                } else {
                    "New windows of these apps are faded the moment they open."
                }),
                button,
            )),
        )
        .margin(Thickness::new(0.0, 4.0, 0.0, 14.0))
        .into()
    }

    fn nav(&self, context: &ViewContext<Self>, page: Page, label: &str, icon: &str, compact: bool) -> View {
        let selected = self.page == page;
        let content: View = if compact {
            glyph(icon, 16.0).into()
        } else {
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(14.0)
                .children((
                    glyph(icon, 16.0).vertical_alignment(VerticalAlignment::Center),
                    TextBlock::new()
                        .text(label)
                        .vertical_alignment(VerticalAlignment::Center),
                ))
                .into()
        };
        let mut button = Button::new()
            .style(ButtonStyle::Subtle)
            .height(40.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .horizontal_content_alignment(if compact {
                HorizontalAlignment::Center
            } else {
                HorizontalAlignment::Left
            })
            .automation_name(label)
            .on_click(context.message(Message::Page(page)))
            .content(content);
        if selected {
            button = button.background(ThemeBrush::SubtleSecondary);
        }
        let pill: View = if selected {
            Border::new()
                .width(3.0)
                .height(16.0)
                .corner_radius(2.0)
                .background(ThemeBrush::Accent)
                .horizontal_alignment(HorizontalAlignment::Left)
                .vertical_alignment(VerticalAlignment::Center)
                .into()
        } else {
            Border::new().into()
        };
        Grid::new().children((button, pill)).into()
    }
}

// ---------------------------------------------------------------------------------------------
// The Apps page.

impl MainWindow {
    fn apps_page(&self, context: &ViewContext<Self>) -> View {
        let header = Grid::new()
            .columns([GridLength::STAR, GridLength::Auto])
            .column_spacing(16.0)
            .children((
                StackPanel::new()
                    .vertical_alignment(VerticalAlignment::Bottom)
                    .children((
                        title("Apps"),
                        caption("Frosted apps first, then the ones you switched off, then everything else that is open."),
                    )),
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(8.0)
                    .vertical_alignment(VerticalAlignment::Bottom)
                    .grid_column(1)
                    .children((
                        AutoSuggestBox::new()
                            .width(240.0)
                            .text(self.search.clone())
                            .placeholder_text("Search apps")
                            .query_icon(Icon::symbol(Symbol::Find))
                            .on_text_changed(context.callback(Message::Search)),
                        Button::new()
                            .is_enabled(false)
                            .content(with_glyph("\u{E7C9}", "Pick a window")),
                    )),
            ));
        let lists = self.entries();
        let mut list: Vec<View> = Vec::new();
        for (index, (label, entries)) in ["Frosted", "Switched off", "Open now"]
            .into_iter()
            .zip(lists.iter())
            .enumerate()
        {
            if entries.is_empty() {
                continue;
            }
            list.push(
                TextBlock::new()
                    .text(format!("{label} · {}", entries.len()))
                    .font_size(12.0)
                    .font_weight(FontWeight::SEMI_BOLD)
                    .foreground(ThemeBrush::SecondaryText)
                    .margin(Thickness::new(16.0, 14.0, 16.0, 4.0))
                    .into(),
            );
            let selected = self.selected.as_ref().and_then(|process| {
                entries
                    .iter()
                    .position(|entry| entry.process.eq_ignore_ascii_case(process))
            });
            list.push(
                ListView::new()
                    .selection_mode(ListViewSelectionMode::Single)
                    .selected_index(selected)
                    .items(
                        entries
                            .iter()
                            .map(|entry| {
                                DataItem::new(entry.process.clone(), entry.process.clone())
                                    .content(self.row(entry, context))
                            })
                            .collect::<Vec<_>>(),
                    )
                    .on_selection_changed(context.callback(move |pick: Option<usize>| {
                        Message::Select(index, pick)
                    }))
                    .into(),
            );
        }
        if list.is_empty() {
            list.push(
                caption(&format!("No open app or rule matches “{}”.", self.search.trim()))
                    .margin(Thickness::new(16.0, 20.0, 16.0, 20.0))
                    .into(),
            );
        }
        let panes = Grid::new()
            .columns([GridLength::Pixel(340.0), GridLength::STAR])
            .column_spacing(24.0)
            .children((
                card(
                    Thickness::new(0.0, 0.0, 0.0, 6.0),
                    StackPanel::new().children(list),
                )
                .vertical_alignment(VerticalAlignment::Top),
                Border::new().grid_column(1).content(self.editor(context)),
            ));
        page_frame(StackPanel::new().spacing(20.0).children((header, panes)))
    }

    fn row(&self, entry: &Entry, context: &ViewContext<Self>) -> View {
        let words = StackPanel::new()
            .vertical_alignment(VerticalAlignment::Center)
            .grid_column(1)
            .children((
                TextBlock::new()
                    .text(entry.process.clone())
                    .text_trimming(TextTrimming::CharacterEllipsis),
                TextBlock::new()
                    .text(entry.subtitle.clone())
                    .font_size(12.0)
                    .foreground(ThemeBrush::SecondaryText)
                    .text_trimming(TextTrimming::CharacterEllipsis),
            ));
        let tag: View = if entry.elevated {
            admin_tag().grid_column(2).into()
        } else {
            Border::new().grid_column(2).into()
        };
        let switch: View = match entry.enabled {
            Some(on) => {
                let process = entry.process.clone();
                ToggleSwitch::new()
                    .is_on(on)
                    .on_content("")
                    .off_content("")
                    .min_width(0.0)
                    .automation_name(format!("Frost {}", entry.process))
                    .on_toggled(context.callback(move |on: bool| Message::Enabled(process.clone(), on)))
                    .grid_column(3)
                    .into()
            }
            None => Border::new().grid_column(3).into(),
        };
        Grid::new()
            .columns([GridLength::Pixel(40.0), GridLength::STAR, GridLength::Auto, GridLength::Auto])
            .column_spacing(8.0)
            .min_height(52.0)
            .automation_name(format!("{}, {}", entry.process, entry.subtitle))
            .children((
                app_icon(self.icon_of(entry.path.as_ref()), 32.0)
                    .vertical_alignment(VerticalAlignment::Center),
                words,
                tag,
                switch,
            ))
            .into()
    }

    fn editor(&self, context: &ViewContext<Self>) -> View {
        let Some(process) = self.selected.clone() else {
            return StackPanel::new()
                .spacing(8.0)
                .margin(Thickness::new(0.0, 80.0, 0.0, 0.0))
                .horizontal_alignment(HorizontalAlignment::Center)
                .children((
                    glyph("\u{E71D}", 40.0)
                        .foreground(ThemeBrush::SecondaryText)
                        .horizontal_alignment(HorizontalAlignment::Center),
                    subtitle_text("Pick an app").horizontal_alignment(HorizontalAlignment::Center),
                    caption("Choose an app on the left to frost it or change its glass.")
                        .horizontal_alignment(HorizontalAlignment::Center),
                ))
                .into();
        };
        let group = self.group(&process);
        let detail = match group {
            Some(group) if group.windows == 1 => format!("{} · 1 window open", group.process),
            Some(group) => format!("{} · {} windows open", group.process, group.windows),
            None => format!("{process} · not running"),
        };
        let rule = self.store.rule(&process);
        let toggle: View = match rule {
            Some(rule) => {
                let target = process.clone();
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(10.0)
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(2)
                    .children((
                        TextBlock::new()
                            .text(if rule.enabled { "Frosted" } else { "Off" })
                            .vertical_alignment(VerticalAlignment::Center),
                        ToggleSwitch::new()
                            .is_on(rule.enabled)
                            .on_content("")
                            .off_content("")
                            .min_width(0.0)
                            .automation_name(format!("Frost {process}"))
                            .on_toggled(context.callback(move |on: bool| Message::Enabled(target.clone(), on))),
                    ))
                    .into()
            }
            None => Border::new().grid_column(2).into(),
        };
        let header = Grid::new()
            .columns([GridLength::Pixel(62.0), GridLength::STAR, GridLength::Auto])
            .children((
                app_icon(self.icon_of(group.and_then(|group| group.path.as_ref())), 48.0)
                    .vertical_alignment(VerticalAlignment::Center),
                StackPanel::new()
                    .vertical_alignment(VerticalAlignment::Center)
                    .grid_column(1)
                    .children((subtitle_text(&process), caption(&detail))),
                toggle,
            ));
        let Some(rule) = rule else {
            let elevated = group.is_some_and(|group| group.elevated);
            let mut parts: Vec<View> = Vec::new();
            if elevated {
                parts.push(
                    InfoBar::new()
                        .title("Runs as administrator")
                        .message("Blurman can frost it only while Blurman runs as administrator too.")
                        .severity(InfoBarSeverity::Warning)
                        .is_open(true)
                        .is_closable(false)
                        .into(),
                );
            }
            parts.push(strong("Not frosted").into());
            parts.push(
                caption(&format!(
                    "Frost it to fade its windows and put glass behind them. New {process} windows get the same look as they open."
                ))
                .into(),
            );
            parts.push(
                Button::new()
                    .style(ButtonStyle::Accent)
                    .is_enabled(!elevated)
                    .on_click(context.message(Message::Frost(process.clone())))
                    .content(format!("Frost {process}"))
                    .into(),
            );
            parts.push(caption("You can change its look right after.").into());
            return StackPanel::new()
                .spacing(16.0)
                .children((header, card(Thickness::new(24.0, 20.0, 24.0, 24.0), StackPanel::new().spacing(12.0).children(parts))))
                .into();
        };
        let tabs = SelectorBar::new()
            .items((
                SelectorBarItem::new().text("Look").is_selected(self.tab == Tab::Look),
                SelectorBarItem::new().text("Finish").is_selected(self.tab == Tab::Finish),
                SelectorBarItem::new().text("Behavior").is_selected(self.tab == Tab::Behavior),
            ))
            .on_selected_text_changed(context.callback(Message::Tab));
        let panel = match self.tab {
            Tab::Look => self.look_rows(rule, context),
            Tab::Finish => self.finish_rows(rule, context),
            Tab::Behavior => self.behavior_rows(rule, context),
        };
        let remove = StackPanel::new()
            .orientation(Orientation::Horizontal)
            .spacing(12.0)
            .children((
                Button::new()
                    .content("Remove rule…")
                    .on_click(context.message(Message::AskDelete))
                    .content_dialog(
                        ContentDialog::new()
                            .title(format!("Remove the rule for {process}?"))
                            .content("The app goes back to normal. You can frost it again any time.")
                            .primary_button_text("Remove")
                            .close_button_text("Cancel")
                            .default_button(ContentDialogButton::Close)
                            .is_open(self.asking_delete)
                            .on_closed(context.callback(Message::DeleteClosed)),
                    ),
                caption(&format!("{process} goes back to normal. Blurman asks first."))
                    .vertical_alignment(VerticalAlignment::Center),
            ));
        StackPanel::new()
            .spacing(16.0)
            .max_width(760.0)
            .horizontal_alignment(HorizontalAlignment::Stretch)
            .children((
                header,
                self.preview_block(rule),
                tabs,
                card(Thickness::new(0.0, 0.0, 0.0, 0.0), panel),
                remove,
            ))
            .into()
    }

    /// Sample shapes behind a pane of the app's glass (Blurman's own frost brush, drawn by WinUI),
    /// with the app faded over the pane as much as its transparency says.
    fn preview_block(&self, rule: &Rule) -> View {
        let dark = self.dark();
        let (desk, app_fill, line) = if dark {
            (Color::rgb(0x22, 0x31, 0x4D), Color::rgb(0x20, 0x20, 0x20), Color::rgb(0x4A, 0x4F, 0x57))
        } else {
            (Color::rgb(0xB9, 0xC9, 0xE4), Color::rgb(0xFF, 0xFF, 0xFF), Color::rgb(0xC9, 0xCE, 0xD6))
        };
        let ink = Color::rgb(0x1F, 0x29, 0x37);
        let blob = |size: f64, color: Color| Ellipse::new().width(size).height(size).fill(color);
        let bar = |width: f64, color: Color| {
            Border::new()
                .width(width)
                .height(9.0)
                .corner_radius(4.5)
                .background(color)
        };
        let app = Border::new()
            .grid_column(1)
            .margin(Thickness::new(0.0, 22.0, 0.0, 0.0))
            .corner_radius(8.0)
            .background(app_fill)
            .opacity(1.0 - rule.look.transparency as f64 / 100.0)
            .padding(Thickness::new(16.0, 14.0, 16.0, 0.0))
            .content(StackPanel::new().spacing(10.0).children((
                Grid::new().columns([GridLength::STAR, GridLength::Auto]).children((
                    bar(64.0, line).horizontal_alignment(HorizontalAlignment::Left),
                    bar(30.0, line).grid_column(1),
                )),
                bar(240.0, line).horizontal_alignment(HorizontalAlignment::Left),
                bar(180.0, line).horizontal_alignment(HorizontalAlignment::Left),
                bar(210.0, line).horizontal_alignment(HorizontalAlignment::Left),
            )));
        Border::new()
            .height(150.0)
            .corner_radius(7.0)
            .border_brush(ThemeBrush::CardStroke)
            .border_thickness(1.0)
            .background(desk)
            .automation_name(format!("Preview of the glass behind {}", rule.process))
            .content(
                Grid::new()
                    .columns([GridLength::Star(1.0), GridLength::Star(4.0), GridLength::Star(1.0)])
                    .children((
                        blob(120.0, Color::rgb(0xF5, 0x9E, 0x0B))
                            .grid_column_span(2)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(28.0, 8.0, 0.0, 0.0)),
                        blob(96.0, Color::rgb(0x14, 0xB8, 0xA6))
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Center)
                            .vertical_alignment(VerticalAlignment::Bottom)
                            .margin(Thickness::new(80.0, 0.0, 0.0, 6.0)),
                        blob(136.0, Color::rgb(0x8B, 0x5C, 0xF6))
                            .grid_column(1)
                            .grid_column_span(2)
                            .horizontal_alignment(HorizontalAlignment::Right)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(0.0, 6.0, 20.0, 0.0)),
                        bar(200.0, ink)
                            .opacity(0.55)
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(30.0, 34.0, 0.0, 0.0)),
                        bar(140.0, ink)
                            .opacity(0.55)
                            .grid_column(1)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Top)
                            .margin(Thickness::new(60.0, 100.0, 0.0, 0.0)),
                        // The glass: WinUI puts the preview's visual on this element.
                        Grid::new()
                            .element_ref(&self.preview_host)
                            .grid_column(1)
                            .margin(Thickness::new(0.0, 22.0, 0.0, 0.0)),
                        app,
                        Border::new()
                            .grid_column_span(3)
                            .horizontal_alignment(HorizontalAlignment::Left)
                            .vertical_alignment(VerticalAlignment::Bottom)
                            .margin(Thickness::new(10.0, 0.0, 0.0, 8.0))
                            .padding(Thickness::new(6.0, 0.0, 6.0, 1.0))
                            .corner_radius(4.0)
                            .background(ThemeBrush::CardBackground)
                            .content(TextBlock::new().text("Preview").font_size(12.0)),
                    )),
            )
            .into()
    }

    fn look_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let look = rule.look;
        let acrylic = look.style == BlurStyle::Acrylic;
        let preset = self.settings.presets.iter().position(|preset| preset.look == look);
        rows(vec![
            row(
                "Glass",
                "Frost blurs what is behind the app. Acrylic is Windows' own milky glass.",
                RadioButtons::new()
                    .width(260.0)
                    .items_source(["Frost", "Acrylic"])
                    .max_columns(2)
                    .selected_index(Some(if acrylic { 1 } else { 0 }))
                    .on_selection_changed(context.callback(Message::Style)),
            ),
            row(
                "Transparency",
                "How much shows through. Text fades too, so it stops at 70%.",
                slider(look.transparency, mapping::TRANSPARENCY_MIN, mapping::TRANSPARENCY_MAX, "%", "Transparency", context.callback(Message::Transparency)),
            ),
            row(
                if acrylic { "Milkiness" } else { "Blur" },
                if acrylic { "How milky the acrylic is." } else { "How soft what shows through is." },
                slider(look.blur, mapping::BLUR_MIN, mapping::BLUR_MAX, "", if acrylic { "Milkiness" } else { "Blur" }, context.callback(Message::Blur)),
            ),
            row(
                "Preset",
                "Start from a saved look.",
                ComboBox::new()
                    .min_width(200.0)
                    .items_source(self.settings.presets.iter().map(|preset| preset.name.clone()))
                    .selected_index(preset)
                    .placeholder_text("Your own look")
                    .on_selection_changed(context.callback(Message::Preset)),
            ),
        ])
    }

    fn finish_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let finish = rule.look.finish;
        let source = TINTS.iter().position(|source| *source == finish.tint_source);
        rows(vec![
            row(
                "Tint",
                "A color laid over the glass: one you pick, or the Windows theme or accent color.",
                StackPanel::new()
                    .orientation(Orientation::Horizontal)
                    .spacing(12.0)
                    .children((
                        ComboBox::new()
                            .min_width(150.0)
                            .items_source(["Your color", "Windows theme", "Accent color"])
                            .selected_index(source)
                            .on_selection_changed(context.callback(Message::TintSource)),
                        slider_sized(finish.tint_amount, 0, mapping::TINT_MAX, "%", "Tint amount", 120.0, context.callback(Message::TintAmount)),
                    )),
            ),
            row(
                "Saturation",
                "Grey at 0, as it is at 100, vivid at 200.",
                slider(finish.saturation, mapping::SATURATION_MIN, mapping::SATURATION_MAX, "", "Saturation", context.callback(Message::Saturation)),
            ),
            row(
                "Milk",
                "Lifts what shows through to a bright, milky glass and keeps its color, for apps in light mode.",
                slider(finish.milk, 0, mapping::MILK_MAX, "", "Milk", context.callback(Message::Milk)),
            ),
            row(
                "Contrast",
                "Punchier glass.",
                slider(finish.contrast, 0, mapping::CONTRAST_MAX, "", "Contrast", context.callback(Message::Contrast)),
            ),
            row(
                "Grain",
                "Fine noise, like real frosted glass.",
                slider(finish.grain, 0, mapping::GRAIN_MAX, "", "Grain", context.callback(Message::Grain)),
            ),
        ])
    }

    fn behavior_rows(&self, rule: &Rule, context: &ViewContext<Self>) -> View {
        let focus = FOCUSES.iter().position(|focus| *focus == rule.focus);
        let chips = |titles: &[String], remove: fn(usize) -> Message, verb: &str| -> View {
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(6.0)
                .margin(Thickness::new(0.0, 10.0, 0.0, 0.0))
                .children(
                    titles
                        .iter()
                        .enumerate()
                        .map(|(index, title)| {
                            Button::new()
                                .automation_name(format!("{verb} {title}"))
                                .on_click(context.message(remove(index)))
                                .content(
                                    StackPanel::new()
                                        .orientation(Orientation::Horizontal)
                                        .spacing(8.0)
                                        .children((
                                            TextBlock::new().text(title.clone()),
                                            glyph("\u{E711}", 10.0).vertical_alignment(VerticalAlignment::Center),
                                        )),
                                )
                                .into()
                        })
                        .collect::<Vec<View>>(),
                )
                .into()
        };
        let adder = |draft: &str, on_text: fn(Rc<str>) -> Message, add: Message, label: &str| -> View {
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((
                    TextBox::new()
                        .width(180.0)
                        .text(draft.to_string())
                        .placeholder_text("Title contains…")
                        .automation_name(label)
                        .on_text_changed(context.callback(on_text)),
                    Button::new().content("Add").on_click(context.message(add)),
                ))
                .into()
        };
        rows(vec![
            row(
                "While you use it",
                "Lighter or solid while the app has focus, fading back to frosted when you move on.",
                RadioButtons::new()
                    .width(380.0)
                    .items_source(["Same", "Lighter", "Solid"])
                    .max_columns(3)
                    .selected_index(focus)
                    .on_selection_changed(context.callback(Message::Focus)),
            ),
            row_with(
                "Only these windows",
                "Frost only windows whose title has one of these words. Empty frosts every window.",
                chips(&rule.only_titles, Message::RemoveOnly, "Stop limiting to"),
                adder(&self.only_draft, Message::OnlyDraft, Message::AddOnly, "Title for only these windows"),
            ),
            row_with(
                "Skip these windows",
                "Leave windows with these words in their title solid, like every Chrome window except YouTube. Titles are checked again as they change.",
                chips(&rule.skip_titles, Message::RemoveSkip, "Stop skipping"),
                adder(&self.skip_draft, Message::SkipDraft, Message::AddSkip, "Title to skip"),
            ),
        ])
    }
}

// ---------------------------------------------------------------------------------------------
// The Settings page, and the pages not built in this spike.

impl MainWindow {
    fn settings_page(&self, context: &ViewContext<Self>) -> View {
        let appearance = &self.settings.appearance;
        let theme = THEMES.iter().position(|name| *name == appearance.theme).unwrap_or(0);
        let glass = GLASSES.iter().position(|name| *name == appearance.window).unwrap_or(0);
        let accent_name = if crate::theme::ACCENTS
            .iter()
            .any(|(name, ..)| name.eq_ignore_ascii_case(&appearance.accent))
        {
            appearance.accent.clone()
        } else {
            "Follows Windows".to_string()
        };
        let mut swatches: Vec<View> = vec![self.swatch(context, "Windows", ThemeBrush::Accent.into(), "Windows accent color")];
        for (name, _, light) in crate::theme::ACCENTS {
            swatches.push(self.swatch(context, name, Color::rgb(light[0], light[1], light[2]).into(), name));
        }
        let groups = StackPanel::new().spacing(22.0).children((
            StackPanel::new().children((
                title("Settings"),
                caption("For Blurman itself. Each app's glass is set on the Apps page."),
            )),
            section(
                "Appearance",
                vec![
                    row(
                        "Theme",
                        "Light, dark, or whatever Windows is set to",
                        ComboBox::new()
                            .min_width(200.0)
                            .items_source(["Use Windows setting", "Light", "Dark"])
                            .selected_index(Some(theme))
                            .on_selection_changed(context.callback(Message::Theme)),
                    ),
                    row(
                        "Accent color",
                        &format!("{accent_name}. For switches, sliders and the page you are on."),
                        StackPanel::new()
                            .orientation(Orientation::Horizontal)
                            .spacing(2.0)
                            .children(swatches),
                    ),
                    row(
                        "Window glass",
                        "Stays glass while you use other apps. Solid over Remote Desktop, in high contrast, or with Windows' transparency effects off.",
                        ComboBox::new()
                            .min_width(200.0)
                            .items_source(["Mica", "Mica Alt", "Acrylic"])
                            .selected_index(Some(glass))
                            .on_selection_changed(context.callback(Message::Glass)),
                    ),
                    row(
                        "Compact sidebar",
                        "Only the pages' icons",
                        switch(appearance.compact_sidebar, "Compact sidebar", context.callback(Message::Compact)),
                    ),
                ],
            ),
            section(
                "Startup and tray",
                vec![
                    row(
                        "Start with Windows",
                        "Starts quietly in the tray when you sign in, so your apps are frosted from the start",
                        switch(self.startup, "Start with Windows", context.callback(Message::Startup)),
                    ),
                    row(
                        "Keep in tray when closed",
                        "Apps stay frosted while the window is closed. The tray menu opens Blurman and pauses it.",
                        switch(self.settings.close_to_tray, "Keep in tray when closed", context.callback(Message::Tray)),
                    ),
                ],
            ),
            section(
                "Updates",
                vec![row(
                    "Check for updates once a day",
                    "Asks GitHub whether a newer Blurman is out and shows a link. Nothing is downloaded. The Microsoft Store version updates itself.",
                    StackPanel::new()
                        .orientation(Orientation::Horizontal)
                        .spacing(16.0)
                        .children((
                            Button::new().content("Check now").is_enabled(false),
                            switch(self.settings.check_updates, "Check for updates once a day", context.callback(Message::Updates)),
                        )),
                )],
            ),
            section(
                "Rules and presets",
                vec![
                    row(
                        "Export",
                        "Your rules and presets to a file, to move them to another PC",
                        Button::new().content("Export…").is_enabled(false),
                    ),
                    row(
                        "Import",
                        "Adds the rules and presets in a file to yours",
                        Button::new().content("Import…").is_enabled(false),
                    ),
                    row(
                        "Where they are kept",
                        &rules::rules_path()
                            .parent()
                            .map(|folder| folder.display().to_string())
                            .unwrap_or_default(),
                        Button::new()
                            .on_click(context.message(Message::OpenFolder))
                            .content(with_glyph("\u{E838}", "Open folder")),
                    ),
                    row(
                        "Delete every rule",
                        "Every app goes back to normal. Your presets stay. Blurman asks first.",
                        Button::new()
                            .content("Delete all rules…")
                            .on_click(context.message(Message::AskDeleteAll))
                            .content_dialog(
                                ContentDialog::new()
                                    .title("Delete every rule?")
                                    .content("Every app goes back to normal, and Blurman forgets how you frosted them. Your presets stay.")
                                    .primary_button_text("Delete all")
                                    .close_button_text("Cancel")
                                    .default_button(ContentDialogButton::Close)
                                    .is_open(self.asking_delete_all)
                                    .on_closed(context.callback(Message::DeleteAllClosed)),
                            ),
                    ),
                ],
            ),
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(4.0)
                .children((
                    caption(&format!("Blurman {} · MIT license ·", env!("CARGO_PKG_VERSION")))
                        .vertical_alignment(VerticalAlignment::Center),
                    HyperlinkButton::new()
                        .content("Source on GitHub")
                        .navigate_uri("https://github.com/xnixy/blurman")
                        .unwrap_or_else(|_| HyperlinkButton::new().content("Source on GitHub")),
                )),
        ));
        page_frame(groups.max_width(1000.0))
    }

    fn swatch(&self, context: &ViewContext<Self>, name: &str, brush: Brush, label: &str) -> View {
        let chosen = if name == "Windows" {
            !crate::theme::ACCENTS
                .iter()
                .any(|(accent, ..)| accent.eq_ignore_ascii_case(&self.settings.appearance.accent))
        } else {
            self.settings.appearance.accent.eq_ignore_ascii_case(name)
        };
        // A Button keeps its own padding around its content, so the swatch is left to size it.
        let ring = Border::new()
            .width(24.0)
            .height(24.0)
            .corner_radius(12.0)
            .border_thickness(2.0)
            .padding(2.0)
            .content(Ellipse::new().width(16.0).height(16.0).fill(brush));
        Button::new()
            .style(ButtonStyle::Subtle)
            .automation_name(label)
            .on_click(context.message(Message::Accent(name.to_string())))
            .content(if chosen {
                ring.border_brush(ThemeBrush::PrimaryText)
            } else {
                ring
            })
            .into()
    }

    fn shortcuts_page(&self, context: &ViewContext<Self>) -> View {
        let shortcut = if self.recording {
            "Press the keys for the shortcut. Esc cancels.".to_string()
        } else {
            match &self.recorded {
                Some(text) => format!("Recorded: {text}"),
                None => "Nothing recorded yet.".to_string(),
            }
        };
        page_frame(StackPanel::new().spacing(22.0).max_width(1000.0).children((
            StackPanel::new().children((
                title("Shortcuts"),
                caption("Not part of this spike, apart from this check that a shortcut can be recorded."),
            )),
            section(
                "Recorder",
                vec![row(
                    "Record a shortcut",
                    &shortcut,
                    Button::new()
                        .content("Record")
                        .is_enabled(!self.recording)
                        .on_click(context.message(Message::Record)),
                )],
            ),
        )))
    }
}

fn placeholder_page(heading: &str, words: &str) -> View {
    page_frame(StackPanel::new().children((title(heading), caption(words))))
}

fn page_key(page: Page) -> &'static str {
    match page {
        Page::Apps => "apps",
        Page::Presets => "presets",
        Page::Shortcuts => "shortcuts",
        Page::AutoPause => "auto-pause",
        Page::Settings => "settings",
    }
}

// ---------------------------------------------------------------------------------------------
// The pieces every page is made of, after the design boards (and remoteman's Views.cs).

/// A page: scrolls, with the boards' margins.
fn page_frame(content: impl Into<View>) -> View {
    ScrollViewer::new()
        .content(
            Border::new()
                .padding(Thickness::new(32.0, 4.0, 32.0, 28.0))
                .content(content),
        )
        .into()
}

/// A page title: WinUI's Title style.
fn title(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(DISPLAY_FONT)
        .font_size(28.0)
        .font_weight(FontWeight::SEMI_BOLD)
}

/// WinUI's Subtitle style.
fn subtitle_text(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_family(DISPLAY_FONT)
        .font_size(20.0)
        .font_weight(FontWeight::SEMI_BOLD)
        .text_trimming(TextTrimming::CharacterEllipsis)
}

/// WinUI's BodyStrong style: section headings.
fn strong(text: &str) -> TextBlock {
    TextBlock::new().text(text).font_weight(FontWeight::SEMI_BOLD)
}

/// WinUI's Caption style in the secondary text color.
fn caption(text: &str) -> TextBlock {
    TextBlock::new()
        .text(text)
        .font_size(12.0)
        .foreground(ThemeBrush::SecondaryText)
        .text_wrapping(TextWrapping::Wrap)
}

fn glyph(code: &str, size: f64) -> FontIcon {
    FontIcon::new().glyph(code).font_size(size)
}

/// A button's words with an icon in front.
fn with_glyph(code: &str, text: &str) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(8.0)
        .children((
            glyph(code, 14.0).vertical_alignment(VerticalAlignment::Center),
            TextBlock::new().text(text),
        ))
        .into()
}

/// A card: the boards' fill, hairline and corners.
fn card(padding: Thickness, content: impl Into<View>) -> Border {
    Border::new()
        .background(ThemeBrush::CardBackground)
        .border_brush(ThemeBrush::CardStroke)
        .border_thickness(1.0)
        .corner_radius(7.0)
        .padding(padding)
        .content(content)
}

/// A heading over a card of rows.
fn section(heading: &str, items: Vec<View>) -> View {
    StackPanel::new()
        .spacing(8.0)
        .children((strong(heading), card(Thickness::new(0.0, 0.0, 0.0, 0.0), rows(items))))
        .into()
}

/// Rows, with a hairline between each.
fn rows(items: Vec<View>) -> View {
    let mut children: Vec<View> = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if index > 0 {
            children.push(Border::new().height(1.0).background(ThemeBrush::CardStroke).into());
        }
        children.push(item);
    }
    StackPanel::new().children(children).into()
}

/// A row: its title and what it says under that on the left, its control on the right.
fn row(heading: &str, words: &str, control: impl Into<View>) -> View {
    row_with(heading, words, Border::new().into(), control.into())
}

/// A row with something more under its words (the title lists' chips).
fn row_with(heading: &str, words: &str, under: View, control: View) -> View {
    Border::new()
        .padding(Thickness::new(16.0, 10.0, 16.0, 10.0))
        .min_height(62.0)
        .content(
            Grid::new()
                .columns([GridLength::STAR, GridLength::Auto])
                .column_spacing(16.0)
                .children((
                    StackPanel::new()
                        .vertical_alignment(VerticalAlignment::Center)
                        .children((
                            TextBlock::new().text(heading).text_wrapping(TextWrapping::Wrap),
                            caption(words),
                            under,
                        )),
                    Border::new()
                        .grid_column(1)
                        .vertical_alignment(VerticalAlignment::Center)
                        .content(control),
                )),
        )
        .into()
}

/// A switch without WinUI's On and Off words beside it: the row says what it is.
fn switch(on: bool, label: &str, toggled: Callback<bool>) -> View {
    ToggleSwitch::new()
        .is_on(on)
        .on_content("")
        .off_content("")
        .min_width(0.0)
        .automation_name(label)
        .on_toggled(toggled)
        .into()
}

/// A slider with its value written after it.
fn slider(value: u8, min: u8, max: u8, unit: &str, label: &str, changed: Callback<f64>) -> View {
    slider_sized(value, min, max, unit, label, 200.0, changed)
}

fn slider_sized(value: u8, min: u8, max: u8, unit: &str, label: &str, width: f64, changed: Callback<f64>) -> View {
    StackPanel::new()
        .orientation(Orientation::Horizontal)
        .spacing(12.0)
        .children((
            Slider::new()
                .width(width)
                .minimum(min as f64)
                .maximum(max as f64)
                .step_frequency(1.0)
                .value(value as f64)
                .automation_name(label)
                .vertical_alignment(VerticalAlignment::Center)
                .on_value_changed(changed),
            TextBlock::new()
                .text(format!("{value}{unit}"))
                .width(40.0)
                .text_alignment(TextAlignment::Right)
                .foreground(ThemeBrush::SecondaryText)
                .vertical_alignment(VerticalAlignment::Center),
        ))
        .into()
}

/// The exe's own icon, or a plain tile where it has none.
fn app_icon(png: Option<EncodedImage>, size: f64) -> Border {
    match png {
        Some(png) => Border::new()
            .width(size)
            .height(size)
            .content(Image::new().source_data(png).width(size).height(size)),
        None => Border::new()
            .width(size)
            .height(size)
            .corner_radius(size / 4.0)
            .background(ThemeBrush::SubtleSecondary)
            .content(
                glyph("\u{E71D}", size / 2.0)
                    .foreground(ThemeBrush::SecondaryText)
                    .horizontal_alignment(HorizontalAlignment::Center)
                    .vertical_alignment(VerticalAlignment::Center),
            ),
    }
}

/// The badge on an app that runs as administrator.
fn admin_tag() -> Border {
    Border::new()
        .border_brush(ThemeBrush::SystemCaution)
        .border_thickness(1.0)
        .corner_radius(4.0)
        .padding(Thickness::new(6.0, 0.0, 6.0, 1.0))
        .vertical_alignment(VerticalAlignment::Center)
        .content(
            TextBlock::new()
                .text("Admin")
                .font_size(11.0)
                .foreground(ThemeBrush::SystemCaution),
        )
}

fn clamp(value: f64, min: u8, max: u8) -> u8 {
    value.round().clamp(min as f64, max as f64) as u8
}

fn subtitle(group: Option<&AppGroup>) -> String {
    match group {
        Some(group) if group.windows > 1 => {
            format!("{} windows · {}", group.windows, group.sample_title)
        }
        Some(group) => group.sample_title.clone(),
        None => "Not running".to_string(),
    }
}

/// The exe's icon as a 48 px PNG, for an Image.
fn icon_png(path: &str) -> Option<EncodedImage> {
    let icon = crate::icons::exe_icon(path, 48)?;
    let pixels = image::RgbaImage::from_raw(icon.size, icon.size, icon.rgba)?;
    let mut bytes = Vec::new();
    pixels
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .ok()?;
    Some(EncodedImage::new(bytes))
}

/// "Ctrl+Alt+B" for a key press with at least one modifier, or None for a lone modifier key or
/// a key a shortcut cannot use.
fn shortcut_text(info: &KeyEventInfo) -> Option<String> {
    let vk = info.key.0;
    let name = match vk {
        0x41..=0x5A | 0x30..=0x39 => char::from_u32(vk)?.to_string(),
        0x60..=0x69 => format!("Num{}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        _ => return None,
    };
    let mods = info.modifiers;
    let mut parts = Vec::new();
    if mods.contains(InputModifiers::WINDOWS) {
        parts.push("Win");
    }
    if mods.contains(InputModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if mods.contains(InputModifiers::ALT) {
        parts.push("Alt");
    }
    if mods.contains(InputModifiers::SHIFT) {
        parts.push("Shift");
    }
    if parts.is_empty() {
        return None;
    }
    Some(format!("{}+{name}", parts.join("+")))
}
