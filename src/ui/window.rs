//! The window: what it keeps, what changes it, and the frame every page sits in (the title bar
//! and the sidebar, with what Blurman is doing at its top).

use super::parts::*;
use super::{icon_files, WindowInput};
use crate::ipc;
use crate::mapping::{self, BlurStyle, FocusLook, Look, Rgb, TintSource};
use crate::rules::{self, HotkeyAction, Preset, Rule, Settings, Store};
use crate::shared::{Shared, Tweak};
use crate::target::{self, AppGroup};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;
use windows_reactor::*;

/// The title bar's icon, at 16 px: the small variant (no window bar) reads better there.
const ICON_PNG: &[u8] = include_bytes!("../../assets/icon-small-32.png");
/// How often the app list is read again.
const RESCAN: Duration = Duration::from_secs(1);
/// How long a slider rests before its rule is written to disk.
const SAVE_AFTER: Duration = Duration::from_millis(600);
/// How long a short message stays at the bottom of the window.
const TOAST_FOR: Duration = Duration::from_secs(5);

pub(super) const THEMES: [&str; 3] = ["windows", "light", "dark"];
pub(super) const GLASSES: [&str; 3] = ["mica", "mica-alt", "acrylic"];
pub(super) const TINTS: [TintSource; 3] = [TintSource::Custom, TintSource::Theme, TintSource::Accent];
pub(super) const FOCUSES: [FocusLook; 3] = [FocusLook::Same, FocusLook::Lighter, FocusLook::Solid];

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Page {
    Apps,
    Presets,
    Shortcuts,
    AutoPause,
    Settings,
}

impl Page {
    fn key(self) -> &'static str {
        match self {
            Page::Apps => "apps",
            Page::Presets => "presets",
            Page::Shortcuts => "shortcuts",
            Page::AutoPause => "auto-pause",
            Page::Settings => "settings",
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Tab {
    Look,
    Finish,
    Behavior,
}

/// One row of the app list: from a rule, an open app, or both.
pub(super) struct Entry {
    pub(super) process: String,
    pub(super) subtitle: String,
    pub(super) path: Option<String>,
    /// Some(on) when the app has a rule.
    pub(super) enabled: Option<bool>,
    pub(super) elevated: bool,
}

/// A dialog asking for a name: renaming the preset at an index, or saving the look as a preset.
#[derive(Clone, PartialEq)]
pub(super) enum Naming {
    Rename(usize),
    Save,
}

pub struct MainWindow {
    pub(super) shared: Arc<Shared>,
    pub(super) store: Store,
    pub(super) settings: Settings,
    /// The settings as they are on disk, to save only real changes.
    pub(super) settings_on_disk: Settings,
    pub(super) startup: bool,
    pub(super) groups: Vec<AppGroup>,
    /// PNG of each exe's icon at 48 px, by path; None when it has none.
    pub(super) icons: HashMap<String, Option<EncodedImage>>,
    pub(super) app_icon: EncodedImage,
    pub(super) search: String,
    pub(super) selected: Option<String>,
    pub(super) page: Page,
    pub(super) tab: Tab,
    pub(super) seen_generation: u64,
    pub(super) seen_settings: u64,
    /// A slider moved and its rule is not on disk yet.
    pub(super) unsaved: bool,
    pub(super) asking_delete: bool,
    pub(super) asking_delete_all: bool,
    pub(super) asking_delete_preset: Option<usize>,
    pub(super) naming: Option<Naming>,
    pub(super) name_draft: String,
    pub(super) asking_tint: bool,
    pub(super) tint_draft: Rgb,
    pub(super) only_draft: String,
    pub(super) skip_draft: String,
    /// The shortcut being recorded.
    pub(super) recording: Option<HotkeyAction>,
    pub(super) toast: Option<String>,
    pub(super) hwnd: isize,
    /// Where the window opened (its last place, if still on a monitor) and where it is now.
    pub(super) start_placement: Option<WindowPlacement>,
    pub(super) placement: Option<WindowPlacement>,
    /// The element the glass preview's visual sits in, and the preview once WinUI has it.
    pub(super) preview_host: ElementRef<Grid>,
    pub(super) preview: Option<crate::preview::Preview>,
    _tick: Option<ComponentTimer>,
    _save: Option<ComponentTimer>,
    _toast: Option<ComponentTimer>,
}

#[derive(Clone)]
pub enum Message {
    Tick,
    Hwnd(isize),
    Placement(WindowPlacement),
    Page(Page),
    Tab(Option<Rc<str>>),
    Search(Rc<str>),
    PickWindow,
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
    AskTint,
    TintColor(Color),
    TintClosed(ContentDialogResult),
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
    ApplyPreset(usize),
    AskName(Naming),
    NameDraft(Rc<str>),
    NameClosed(ContentDialogResult),
    AskDeletePreset(usize),
    DeletePresetClosed(ContentDialogResult),
    UseShortcuts(bool),
    Record(HotkeyAction),
    ClearShortcut(HotkeyAction),
    /// A key pressed while a shortcut is being recorded: its virtual-key code and modifiers.
    Key {
        vk: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
    },
    BatterySaver(bool),
    Quiet(bool),
    Schedule(bool),
    From(Rc<str>),
    Until(Rc<str>),
    Theme(Option<usize>),
    Glass(Option<usize>),
    Compact(bool),
    Startup(bool),
    Tray(bool),
    Updates(bool),
    CheckNow,
    OpenStore,
    OpenFolder,
    AskDeleteAll,
    DeleteAllClosed(ContentDialogResult),
    ToastDone,
    /// WinUI has the preview's element ready, or it changed size.
    PreviewHost(CompositionHostEvent),
}

impl Component for MainWindow {
    type Input = WindowInput;
    type Message = Message;

    fn create(input: &WindowInput, context: &ComponentContext<Self>) -> Self {
        // The window handle, for Win32 work on the window (bringing it forward from a second
        // launch, the window picker). Reactor hands it out on the UI thread.
        let _ = context.run_window(|window| Message::Hwnd(window.as_raw() as isize));
        let shared = input.shared.0.clone();
        let settings = rules::load_settings();
        let mut window = Self {
            seen_generation: shared.rules_generation.load(Ordering::SeqCst),
            seen_settings: shared.settings_generation.load(Ordering::SeqCst),
            shared,
            store: rules::load(),
            settings_on_disk: settings.clone(),
            settings,
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
            asking_delete_preset: None,
            naming: None,
            name_draft: String::new(),
            asking_tint: false,
            tint_draft: Rgb(0, 0, 0),
            only_draft: String::new(),
            skip_draft: String::new(),
            recording: None,
            toast: None,
            hwnd: 0,
            start_placement: saved_placement(),
            placement: None,
            preview_host: ElementRef::new(),
            preview: None,
            _tick: None,
            _save: None,
            _toast: None,
        };
        window.debug_start();
        window.rescan();
        window._tick = Some(context.set_local_timeout(RESCAN, || Message::Tick));
        window
    }

    fn update(&mut self, message: Message, context: &ComponentContext<Self>) {
        match message {
            Message::Tick => {
                self.rescan();
                self.sync_from_disk();
                self.take_news(context);
                if let Some(process) = self.shared.picked.take() {
                    self.flush_drafts(context);
                    self.page = Page::Apps;
                    self.selected = Some(process);
                }
                self._tick = Some(context.set_local_timeout(RESCAN, || Message::Tick));
            }
            Message::Hwnd(hwnd) => {
                self.hwnd = hwnd;
                self.shared.main_window.store(hwnd, Ordering::SeqCst);
            }
            Message::Placement(placement) => self.placement = Some(placement),
            Message::Page(page) => {
                self.flush_drafts(context);
                self.stop_recording();
                self.page = page;
            }
            Message::Tab(text) => {
                self.tab = match text.as_deref() {
                    Some("Finish") => Tab::Finish,
                    Some("Behavior") => Tab::Behavior,
                    _ => Tab::Look,
                }
            }
            Message::Search(text) => self.search = text.to_string(),
            Message::PickWindow => crate::picker::start(self.shared.clone()),
            Message::Select(group, Some(index)) => {
                let picked = self
                    .entries()
                    .into_iter()
                    .nth(group)
                    .and_then(|entries| entries.into_iter().nth(index));
                if let Some(entry) = picked {
                    if self.selected.as_deref() != Some(entry.process.as_str()) {
                        self.flush_drafts(context);
                        self.selected = Some(entry.process);
                    }
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
                let style = if index == 1 {
                    BlurStyle::Acrylic
                } else {
                    BlurStyle::Frost
                };
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
            Message::Preset(Some(index)) | Message::ApplyPreset(index) => {
                if let Some(preset) = self.settings.presets.get(index) {
                    let (name, look) = (preset.name.clone(), preset.look);
                    self.change_look(context, |current| *current = look);
                    if let Some(process) = &self.selected {
                        self.notify(context, format!("{process} now uses {name}"));
                    }
                }
            }
            Message::TintSource(Some(index)) => {
                self.change_look(context, |look| look.finish.tint_source = TINTS[index.min(2)]);
            }
            Message::TintAmount(value) => {
                let value = clamp(value, 0, mapping::TINT_MAX);
                self.change_look(context, |look| look.finish.tint_amount = value);
            }
            Message::AskTint => {
                if let Some(rule) = self.selected_rule() {
                    self.tint_draft = rule.look.finish.tint;
                    self.asking_tint = true;
                }
            }
            Message::TintColor(color) => self.tint_draft = Rgb(color.r, color.g, color.b),
            Message::TintClosed(result) => {
                self.asking_tint = false;
                if matches!(result, ContentDialogResult::Primary) {
                    let tint = self.tint_draft;
                    self.change_look(context, |look| {
                        look.finish.tint = tint;
                        look.finish.tint_source = TintSource::Custom;
                    });
                }
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
            // Several titles can go in at once: "YouTube | Netflix".
            Message::AddOnly => {
                let titles = rules::split_titles(&std::mem::take(&mut self.only_draft));
                if !titles.is_empty() {
                    self.edit_rule(context, |rule| rule.only_titles.extend(titles));
                }
            }
            Message::RemoveOnly(index) => self.edit_rule(context, |rule| {
                if index < rule.only_titles.len() {
                    rule.only_titles.remove(index);
                }
            }),
            Message::SkipDraft(text) => self.skip_draft = text.to_string(),
            Message::AddSkip => {
                let titles = rules::split_titles(&std::mem::take(&mut self.skip_draft));
                if !titles.is_empty() {
                    self.edit_rule(context, |rule| rule.skip_titles.extend(titles));
                }
            }
            Message::RemoveSkip(index) => self.edit_rule(context, |rule| {
                if index < rule.skip_titles.len() {
                    rule.skip_titles.remove(index);
                }
            }),
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
            Message::AskName(naming) => {
                self.name_draft = match &naming {
                    Naming::Rename(index) => self
                        .settings
                        .presets
                        .get(*index)
                        .map(|preset| preset.name.clone())
                        .unwrap_or_default(),
                    Naming::Save => String::new(),
                };
                self.naming = Some(naming);
            }
            Message::NameDraft(text) => self.name_draft = text.to_string(),
            // Esc or Cancel closes the dialog without a name: nothing changes.
            Message::NameClosed(result) => {
                let naming = self.naming.take();
                let name = std::mem::take(&mut self.name_draft).trim().to_string();
                if !matches!(result, ContentDialogResult::Primary) || name.is_empty() {
                    return self.refresh_preview();
                }
                match naming {
                    Some(Naming::Rename(index)) => {
                        let old = self.settings.presets.get(index).map(|preset| preset.name.clone());
                        if let Some(old) = old {
                            match self.settings.rename_preset(&old, &name) {
                                rules::Renamed::Done => self.save_settings(context),
                                rules::Renamed::Taken => self.notify(
                                    context,
                                    "Another preset already has that name".to_string(),
                                ),
                                rules::Renamed::Kept => {}
                            }
                        }
                    }
                    Some(Naming::Save) => {
                        if let Some(look) = self.selected_rule().map(|rule| rule.look) {
                            self.settings.save_preset(Preset {
                                name: name.clone(),
                                look,
                            });
                            self.save_settings(context);
                            self.notify(context, format!("Saved as {name}"));
                        }
                    }
                    None => {}
                }
            }
            Message::AskDeletePreset(index) => self.asking_delete_preset = Some(index),
            Message::DeletePresetClosed(result) => {
                if let (true, Some(index)) = (
                    matches!(result, ContentDialogResult::Primary),
                    self.asking_delete_preset,
                ) {
                    if index < self.settings.presets.len() {
                        self.settings.presets.remove(index);
                        self.save_settings(context);
                    }
                }
                self.asking_delete_preset = None;
            }
            Message::UseShortcuts(on) => {
                self.stop_recording();
                self.settings.hotkeys.enabled = on;
                self.save_settings(context);
            }
            Message::Record(action) => {
                if self.recording == Some(action) {
                    self.stop_recording();
                } else {
                    self.recording = Some(action);
                    // The effect thread lets its shortcuts go, so the keys reach the window.
                    self.shared.recording_shortcut.store(true, Ordering::SeqCst);
                }
            }
            Message::ClearShortcut(action) => {
                self.stop_recording();
                self.settings.hotkeys.set(action, String::new());
                self.save_settings(context);
            }
            Message::Key {
                vk,
                ctrl,
                alt,
                shift,
                win,
            } => self.recorded(context, vk, ctrl, alt, shift, win),
            Message::BatterySaver(on) => {
                self.settings.auto_pause.battery_saver = on;
                self.save_settings(context);
            }
            Message::Quiet(on) => {
                self.settings.auto_pause.quiet = on;
                self.save_settings(context);
            }
            Message::Schedule(on) => {
                self.settings.auto_pause.schedule = on;
                self.save_settings(context);
            }
            // A time is saved only once it reads as one; until then the last good one stays.
            Message::From(text) => {
                self.settings.auto_pause.from = text.trim().to_string();
                self.save_settings(context);
            }
            Message::Until(text) => {
                self.settings.auto_pause.until = text.trim().to_string();
                self.save_settings(context);
            }
            Message::Theme(Some(index)) => {
                self.settings.appearance.theme = THEMES[index.min(2)].into();
                self.save_settings(context);
            }
            Message::Glass(Some(index)) => {
                self.settings.appearance.window = GLASSES[index.min(2)].into();
                self.save_settings(context);
            }
            Message::Compact(on) => {
                self.settings.appearance.compact_sidebar = on;
                self.save_settings(context);
            }
            Message::Startup(on) => {
                if let Err(err) = crate::autostart::set_enabled(on) {
                    self.notify(context, err);
                }
                self.startup = crate::autostart::is_enabled();
            }
            Message::Tray(on) => {
                self.settings.close_to_tray = on;
                self.save_settings(context);
            }
            Message::Updates(on) => {
                self.settings.check_updates = on;
                if !on {
                    self.shared.set_update(None);
                }
                self.save_settings(context);
                // The daily check reads the setting again now, not up to a day later.
                self.shared.wake_update_watcher();
            }
            Message::CheckNow => {
                let shared = self.shared.clone();
                std::thread::spawn(move || match crate::update::check_once(&shared) {
                    Some(Ok(Some(found))) => shared.set_update(Some(found)),
                    Some(Ok(None)) => shared.set_notice(format!(
                        "Blurman {} is the latest",
                        env!("CARGO_PKG_VERSION")
                    )),
                    Some(Err(err)) => shared.set_notice(format!("Could not check: {err}")),
                    None => shared.set_notice("Already checking"),
                });
            }
            Message::OpenStore => crate::system::open(crate::package::STORE_URL),
            Message::OpenFolder => {
                let _ = std::process::Command::new("explorer")
                    .arg(rules::app_dir_on_disk())
                    .spawn();
            }
            Message::AskDeleteAll => self.asking_delete_all = true,
            Message::DeleteAllClosed(result) => {
                self.asking_delete_all = false;
                if matches!(result, ContentDialogResult::Primary) {
                    self.store.rules.clear();
                    self.publish(context);
                }
            }
            Message::ToastDone => self.toast = None,
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
        let mut visuals = WindowVisuals::new()
            .client_size(1180.0, 780.0)
            .constraints(WindowConstraints {
                min_width: Some(720.0),
                min_height: Some(520.0),
                ..Default::default()
            })
            .backdrop(self.backdrop())
            .theme(self.theme())
            .icon(icon_files().app.as_str());
        if let Some(placement) = self.start_placement {
            visuals = visuals.initial_placement(placement);
        }
        context.window_visuals(visuals);
        context.on_window_placement(context.callback(Message::Placement));

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

        let page: View = match self.page {
            Page::Apps => self.apps_page(context),
            Page::Presets => self.presets_page(context),
            Page::Shortcuts => self.shortcuts_page(context),
            Page::AutoPause => self.auto_pause_page(context),
            Page::Settings => self.settings_page(context),
        };
        let toast: View = match &self.toast {
            Some(text) => InfoBar::new()
                .message(text.clone())
                .severity(InfoBarSeverity::Informational)
                .is_open(true)
                .is_closable(true)
                .on_closed(context.message(Message::ToastDone))
                .horizontal_alignment(HorizontalAlignment::Center)
                .vertical_alignment(VerticalAlignment::Bottom)
                .max_width(560.0)
                .margin(Thickness::new(16.0, 0.0, 16.0, 16.0))
                .into(),
            None => Border::new().into(),
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
                Grid::new().grid_column(1).children((
                    Grid::new().keyed_children([keyed(self.page.key(), page)]),
                    toast,
                )),
            ));
        let root = Grid::new()
            .rows([GridLength::Auto, GridLength::STAR])
            .children((title_bar, body));

        // The shortcut recorder: while recording, every key press goes to it first.
        let recording = self.recording.is_some();
        let pressed = context.forward();
        Border::new()
            .on_preview_key_down(RoutedCallback::new(move |info: KeyEventInfo| {
                if !recording {
                    return false;
                }
                let mods = info.modifiers;
                pressed.call(Message::Key {
                    vk: info.key.0,
                    ctrl: mods.contains(InputModifiers::CONTROL),
                    alt: mods.contains(InputModifiers::ALT),
                    shift: mods.contains(InputModifiers::SHIFT),
                    win: mods.contains(InputModifiers::WINDOWS),
                });
                true
            }))
            .content(root)
            .into()
    }
}

impl Drop for MainWindow {
    fn drop(&mut self) {
        // The window is closing (to the tray, or for good): there is no window to bring forward.
        let _ = self.shared.main_window.compare_exchange(
            self.hwnd,
            0,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );
        self.stop_recording();
        // Titles still being typed are kept, as a slider still settling is.
        let only = rules::split_titles(&std::mem::take(&mut self.only_draft));
        let skip = rules::split_titles(&std::mem::take(&mut self.skip_draft));
        if let Some(rule) = self.selected.clone().and_then(|p| self.store.rule_mut(&p)) {
            if !only.is_empty() || !skip.is_empty() {
                rule.only_titles.extend(only);
                rule.skip_titles.extend(skip);
                self.unsaved = true;
            }
        }
        if self.unsaved {
            self.store.normalize();
            if rules::save(&self.store).is_ok() {
                ipc::signal(ipc::msg_reload());
            }
        }
        if let Some(placement) = self.placement {
            save_placement(placement);
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
    /// Only files that read cleanly replace what the window has.
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
            if let Some(loaded) = rules::read_settings() {
                self.settings = loaded.clone();
                self.settings_on_disk = loaded;
                self.seen_settings = settings;
            }
        }
    }

    /// Messages from the other threads: a short note, or a newer Blurman.
    fn take_news(&mut self, context: &ComponentContext<Self>) {
        if let Some(text) = self.shared.notice.take() {
            self.notify(context, text);
        }
    }

    pub(super) fn notify(&mut self, context: &ComponentContext<Self>, text: String) {
        self.toast = Some(text);
        self._toast = Some(context.set_local_timeout(TOAST_FOR, || Message::ToastDone));
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

    /// Titles still being typed are added when you move to another app or page.
    fn flush_drafts(&mut self, context: &ComponentContext<Self>) {
        let only = rules::split_titles(&std::mem::take(&mut self.only_draft));
        let skip = rules::split_titles(&std::mem::take(&mut self.skip_draft));
        if only.is_empty() && skip.is_empty() {
            return;
        }
        self.edit_rule(context, |rule| {
            rule.only_titles.extend(only);
            rule.skip_titles.extend(skip);
        });
    }

    /// Write the rules, have the effect thread reload them, and let the tray know.
    pub(super) fn publish(&mut self, context: &ComponentContext<Self>) {
        self.unsaved = false;
        self.store.normalize();
        if let Err(err) = rules::save(&self.store) {
            self.notify(context, format!("Could not save rules: {err}"));
            return;
        }
        ipc::signal(ipc::msg_reload());
        context.input().changed.call(());
    }

    /// Save the settings if they changed. A schedule time still being typed keeps the last one
    /// that read as a time.
    pub(super) fn save_settings(&mut self, context: &ComponentContext<Self>) {
        let last = self.settings_on_disk.auto_pause.clone();
        let mut to_save = self.settings.clone();
        to_save.auto_pause.keep_times_that_read(&last);
        if to_save == self.settings_on_disk {
            return;
        }
        match rules::save_settings(&to_save) {
            Ok(()) => {
                self.settings_on_disk = to_save;
                ipc::signal(ipc::msg_settings());
                context.input().changed.call(());
            }
            Err(err) => self.notify(context, format!("Could not save settings: {err}")),
        }
    }

    fn stop_recording(&mut self) {
        if self.recording.take().is_some() {
            self.shared.recording_shortcut.store(false, Ordering::SeqCst);
        }
    }

    /// A key pressed while recording `self.recording`. Esc keeps the old shortcut, Backspace or
    /// Delete alone turns it off, and anything that is not yet a shortcut (a lone modifier, a key
    /// without Ctrl, Alt or Win) is waited past. Number pad digits stay number pad keys.
    fn recorded(
        &mut self,
        context: &ComponentContext<Self>,
        vk: u32,
        ctrl: bool,
        alt: bool,
        shift: bool,
        win: bool,
    ) {
        let Some(action) = self.recording else {
            return;
        };
        const ESCAPE: u32 = 0x1b;
        const BACK: u32 = 0x08;
        const DELETE: u32 = 0x2e;
        let modified = ctrl || alt || shift || win;
        let combo = match vk {
            ESCAPE => return self.stop_recording(),
            BACK | DELETE if !modified => String::new(),
            _ => match crate::hotkey::recorded(vk, ctrl, alt, shift, win) {
                Some(combo) => combo,
                None => return,
            },
        };
        let taken = HotkeyAction::ALL.into_iter().find(|other| {
            *other != action
                && !combo.is_empty()
                && self.settings.hotkeys.get(*other).eq_ignore_ascii_case(&combo)
        });
        self.stop_recording();
        match taken {
            Some(other) => self.notify(
                context,
                format!("{combo} is already used for {}", other.label()),
            ),
            None => {
                self.settings.hotkeys.set(action, combo);
                self.save_settings(context);
            }
        }
    }

    pub(super) fn selected_rule(&self) -> Option<&Rule> {
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
    pub(super) fn dark(&self) -> bool {
        match self.settings.appearance.theme.as_str() {
            "dark" => true,
            "light" => false,
            _ => crate::system::system_colors().dark,
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

    pub(super) fn group(&self, process: &str) -> Option<&AppGroup> {
        self.groups
            .iter()
            .find(|group| group.process.eq_ignore_ascii_case(process))
    }

    pub(super) fn icon_of(&self, path: Option<&String>) -> Option<EncodedImage> {
        path.and_then(|path| self.icons.get(path)).cloned().flatten()
    }

    /// Frosted apps, apps whose rule is off, and everything else that is open, each matching the
    /// search.
    pub(super) fn entries(&self) -> [Vec<Entry>; 3] {
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

    /// Debug builds can open straight onto a screen, for checking it and for the README's
    /// pictures: BLURMAN_UI_PAGE, BLURMAN_UI_SELECT (an app) and BLURMAN_UI_TAB.
    fn debug_start(&mut self) {
        if !cfg!(debug_assertions) {
            return;
        }
        let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
        if let Some(page) = var("BLURMAN_UI_PAGE") {
            self.page = match page.as_str() {
                "presets" => Page::Presets,
                "shortcuts" => Page::Shortcuts,
                "auto-pause" => Page::AutoPause,
                "settings" => Page::Settings,
                _ => Page::Apps,
            };
        }
        if let Some(process) = var("BLURMAN_UI_SELECT") {
            self.selected = Some(mapping::normalize_process(&process));
        }
        if let Some(tab) = var("BLURMAN_UI_TAB") {
            self.tab = match tab.as_str() {
                "finish" => Tab::Finish,
                "behavior" => Tab::Behavior,
                _ => Tab::Look,
            };
        }
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

    /// What Blurman is doing right now, with Pause all or Resume, and a newer Blurman if one is
    /// out.
    fn status_card(&self, context: &ViewContext<Self>) -> View {
        let paused = self.store.paused;
        let auto = self.shared.auto_paused.get();
        let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
        let resting = paused || auto.is_some();
        let dot: View = if resting {
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
        let (heading, words) = match (&auto, paused, frosted) {
            (_, true, _) => (
                "Paused".to_string(),
                "Every app is back to normal. Your rules are kept.".to_string(),
            ),
            (Some(reason), false, _) => (
                "Auto-paused".to_string(),
                format!("{reason}. Apps frost again once that ends."),
            ),
            (None, false, 1) => (
                "Frosting 1 app".to_string(),
                "New windows of it are faded the moment they open.".to_string(),
            ),
            (None, false, many) => (
                format!("Frosting {many} apps"),
                "New windows of these apps are faded the moment they open.".to_string(),
            ),
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
        let mut parts: Vec<View> = vec![
            StackPanel::new()
                .orientation(Orientation::Horizontal)
                .spacing(8.0)
                .children((dot, strong(&heading)))
                .into(),
            caption(&words).into(),
            button.into(),
        ];
        // A problem the effect thread ran into: an app it cannot frost, a file it cannot save.
        let status = self.shared.status();
        if !status.is_empty() {
            parts.push(warning(&status).into());
        }
        if let Some(update) = self.shared.update.get() {
            parts.push(
                StackPanel::new()
                    .children((
                        caption(&format!("Blurman {} is out.", update.version)),
                        HyperlinkButton::new()
                            .content("See what's new")
                            .navigate_uri(update.url.clone())
                            .unwrap_or_else(|_| HyperlinkButton::new().content("See what's new")),
                    ))
                    .into(),
            );
        }
        card(
            Thickness::new(14.0, 12.0, 14.0, 12.0),
            StackPanel::new().spacing(10.0).children(parts),
        )
        .margin(Thickness::new(0.0, 4.0, 0.0, 14.0))
        .into()
    }

    fn nav(
        &self,
        context: &ViewContext<Self>,
        page: Page,
        label: &str,
        icon: &str,
        compact: bool,
    ) -> View {
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
// Where the window was.

/// The window's place, as it is kept on disk between runs.
#[derive(Serialize, Deserialize)]
struct SavedPlacement {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    maximized: bool,
}

fn placement_path() -> std::path::PathBuf {
    rules::app_dir_on_disk().join("window.json")
}

/// The window's last place, if it is still on a monitor: one unplugged since, or a resolution
/// change, opens it where Windows puts new windows instead.
fn saved_placement() -> Option<WindowPlacement> {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
    let text = std::fs::read_to_string(placement_path()).ok()?;
    let saved: SavedPlacement = serde_json::from_str(&text).ok()?;
    if saved.width < 200 || saved.height < 200 {
        return None;
    }
    let bounds = RECT {
        left: saved.x,
        top: saved.y,
        right: saved.x + saved.width,
        bottom: saved.y + saved.height,
    };
    if unsafe { MonitorFromRect(&bounds, MONITOR_DEFAULTTONULL) }.is_invalid() {
        return None;
    }
    Some(WindowPlacement {
        x: saved.x,
        y: saved.y,
        width: saved.width,
        height: saved.height,
        maximized: saved.maximized,
    })
}

fn save_placement(placement: WindowPlacement) {
    let saved = SavedPlacement {
        x: placement.x,
        y: placement.y,
        width: placement.width,
        height: placement.height,
        maximized: placement.maximized,
    };
    if let Ok(text) = serde_json::to_string(&saved) {
        let _ = std::fs::write(placement_path(), text);
    }
}
