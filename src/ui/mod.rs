//! Blurman's window and tray icon: native WinUI 3 through Microsoft's windows-reactor, drawn after
//! the "Blurman for Windows" design boards. One Mica surface that stays glass while other apps are
//! in front, a sidebar with what Blurman is doing at its top, and Windows Settings-style rows in
//! cards.
//!
//! The window drives the effect thread as the command line does: rules and settings are saved and
//! the effect thread is signalled, and a slider moving goes out as a live tweak.

mod apps;
mod pages;
mod parts;
mod window;

use crate::ipc;
use crate::mapping::Look;
use crate::rules::{self, Store};
use crate::shared::Shared;
use crate::target;
use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use window::MainWindow;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, IsWindow, SetWindowsHookExW, MSG, PM_REMOVE, WH_GETMESSAGE, WM_APP,
};
use windows_reactor::*;

/// Asks the UI thread to show the window: from a second launch, the tray or a shortcut.
pub const WM_OPEN: u32 = WM_APP + 2;
/// Asks the UI thread to look at the rules again, after the effect thread reloaded them.
pub const WM_SYNC: u32 = WM_APP + 3;

/// The icon, from assets/icon (rendered by assets/icon/render.ps1).
const APP_ICO: &[u8] = include_bytes!("../../assets/blurman.ico");
const TRAY_ICO: &[u8] = include_bytes!("../../assets/tray.ico");
const TRAY_PAUSED_ICO: &[u8] = include_bytes!("../../assets/tray-paused.ico");

thread_local! {
    /// Where the UI thread's thread messages go: the application, while it runs.
    static APPLICATION: RefCell<Option<LocalSender<AppMessage>>> = const { RefCell::new(None) };
}

/// The effect thread and other launches talk to the UI thread with thread messages
/// (`Shared::show_window`, `Shared::sync_tray`). Reactor's message loop has no handler for those,
/// so this hook sees them as they are taken off the queue and tells the application.
unsafe extern "system" fn watch_thread_messages(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 && wparam.0 == PM_REMOVE.0 as usize {
        let message = &*(lparam.0 as *const MSG);
        let wanted = match message.message {
            WM_OPEN => Some(AppMessage::Open),
            WM_SYNC => Some(AppMessage::Changed),
            _ => None,
        };
        if let (true, Some(wanted)) = (message.hwnd.0.is_null(), wanted) {
            APPLICATION.with(|application| {
                if let Some(sender) = application.borrow().as_ref() {
                    let _ = sender.send(wanted);
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
pub struct Handle(pub(super) Arc<Shared>);

impl PartialEq for Handle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

/// Run the window and the tray icon until Exit, or until the window closes while Blurman does
/// not keep to the tray. With `startup`, start in the tray.
pub fn run(shared: Arc<Shared>, startup: bool) -> Result<(), String> {
    shared
        .ui_thread
        .store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);
    App::run_application::<Blurman>((Handle(shared), startup)).map_err(|err| err.to_string())
}

/// The application: one reopenable window, and the tray icon while Blurman keeps to the tray.
struct Blurman {
    shared: Arc<Shared>,
    /// Started at sign-in: lives in the tray whatever the setting says.
    startup: bool,
    store: Store,
    close_to_tray: bool,
}

#[derive(Clone)]
enum AppMessage {
    Open,
    Menu(Key),
    /// The rules or settings changed: look again at what the tray shows.
    Changed,
}

#[derive(Clone, PartialEq)]
pub struct WindowInput {
    pub(super) shared: Handle,
    pub(super) changed: Callback<()>,
}

impl Blurman {
    fn read_again(&mut self) {
        if let Ok(store) = rules::read() {
            self.store = store;
        }
        if let Some(settings) = rules::read_settings() {
            self.close_to_tray = settings.close_to_tray;
        }
    }

    fn tray(&self) -> bool {
        self.startup || self.close_to_tray
    }

    /// Save a change made from the menu and tell the effect thread.
    fn save(&mut self, store: Store) {
        if let Err(err) = rules::save(&store) {
            self.shared.set_notice(format!("Could not save rules: {err}"));
            return;
        }
        ipc::signal(ipc::msg_reload());
        self.store = store;
    }

    /// The rules to change from the menu. A file that cannot be read is left alone and reported:
    /// saving a change over it would lose every rule.
    fn rules_to_edit(&self) -> Option<Store> {
        match rules::read() {
            Ok(store) => Some(store),
            Err(err) => {
                self.shared.set_notice(err);
                None
            }
        }
    }

    /// Opening the menu takes focus from the app you were using, so this acts on the app the
    /// effect thread saw last instead of the window in front.
    fn frost_last_app(&mut self) {
        let hwnd = target::hwnd_of(self.shared.last_app.load(Ordering::SeqCst));
        if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return;
        }
        let Some(process) = target::process_name(target::pid_of(hwnd)) else {
            return;
        };
        if let Some(mut store) = self.rules_to_edit() {
            store.toggle_app(&process, Look::default());
            self.save(store);
        }
    }

    fn tooltip(&self) -> String {
        let frosted = self.store.rules.iter().filter(|rule| rule.enabled).count();
        match (self.store.paused, frosted) {
            (true, _) => "Blurman: paused".to_string(),
            (false, 0) => "Blurman".to_string(),
            (false, 1) => "Blurman: 1 app frosted".to_string(),
            (false, many) => format!("Blurman: {many} apps frosted"),
        }
    }

    fn menu(&self, context: &ApplicationViewContext<Self>) -> Menu {
        let mut items = vec![
            MenuItem::item("open", "Open Blurman"),
            MenuItem::item("pause", if self.store.paused { "Resume" } else { "Pause all" }),
            MenuItem::item("front", "Frost or unfrost the last app"),
        ];
        if !self.store.rules.is_empty() {
            items.push(MenuItem::submenu(
                "apps",
                "Apps",
                self.store.rules.iter().map(|rule| {
                    // The menu has no check marks; a frosted app has one in its label.
                    let label = if rule.enabled {
                        format!("✓  {}", rule.process)
                    } else {
                        format!("     {}", rule.process)
                    };
                    MenuItem::item(format!("rule:{}", rule.process), label)
                }),
            ));
        }
        items.push(MenuItem::separator("separator"));
        items.push(MenuItem::item("exit", "Exit"));
        Menu::new(items, context.callback(AppMessage::Menu))
    }
}

impl Application for Blurman {
    type Input = (Handle, bool);
    type Message = AppMessage;

    fn create(input: &(Handle, bool), context: &ApplicationContext<Self>) -> Self {
        APPLICATION.with(|application| *application.borrow_mut() = Some(context.sender()));
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
        Self {
            shared: input.0 .0.clone(),
            startup: input.1,
            store: rules::load(),
            close_to_tray: rules::load_settings().close_to_tray,
        }
    }

    fn update(&mut self, message: AppMessage, context: &ApplicationContext<Self>) {
        match message {
            AppMessage::Open => context.show_window("main"),
            AppMessage::Changed => self.read_again(),
            AppMessage::Menu(key) => match key.as_str() {
                Some("open") => context.show_window("main"),
                Some("pause") => {
                    if let Some(mut store) = self.rules_to_edit() {
                        store.paused = !store.paused;
                        self.save(store);
                    }
                }
                Some("front") => self.frost_last_app(),
                Some("exit") => {
                    self.shared.exiting.store(true, Ordering::SeqCst);
                    context.exit();
                }
                Some(other) => {
                    if let Some(process) = other.strip_prefix("rule:") {
                        if let Some(mut store) = self.rules_to_edit() {
                            if let Some(rule) = store.rule_mut(process) {
                                rule.enabled = !rule.enabled;
                                self.save(store);
                            }
                        }
                    }
                }
                None => {}
            },
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
        if !self.tray() {
            return view;
        }
        let files = icon_files();
        view.notify_icon(
            "tray",
            NotifyIcon::new(if self.store.paused { &files.paused } else { &files.tray })
                .tooltip(self.tooltip())
                .on_activate(context.callback(|_| AppMessage::Open))
                .menu(self.menu(context)),
        )
    }
}
