//! The tray icon, present while "Keep in tray on close" is on. Lives on the UI thread.

use crate::ipc;
use crate::mapping::Look;
use crate::rules::{self, Store};
use crate::shared::Shared;
use crate::target;
use std::cell::RefCell;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsWindow, PostMessageW, PostThreadMessageW, TranslateMessage,
    MSG, WM_APP, WM_CLOSE,
};

/// Posted to the UI thread to open the window while Blurman waits in the tray.
pub const WM_OPEN: u32 = WM_APP + 2;
/// Posted to the UI thread when the rules changed somewhere other than the window.
pub const WM_SYNC: u32 = WM_APP + 3;
/// Posted to the UI thread by Exit in the menu.
const WM_EXIT: u32 = WM_APP + 4;

// Menu handlers run inside muda's WM_COMMAND, which holds the clicked item borrowed. Dropping or
// replacing the menu there borrows every item again, panics inside a window procedure, and
// aborts the process. So handlers only save and post; the menu changes once they return.

const OPEN: &str = "open";
const PAUSE: &str = "pause";
const FRONT: &str = "front";
const EXIT: &str = "exit";
/// Prefix of the per-app items, followed by the process name.
const RULE: &str = "rule:";

struct Tray {
    icon: TrayIcon,
    pause: CheckMenuItem,
    /// What the menu was built from. A different list of apps rebuilds it.
    apps: Vec<(String, bool)>,
}

thread_local! {
    static TRAY: RefCell<Option<Tray>> = const { RefCell::new(None) };
}

/// Run the tray until something asks for the window. Returns false if the thread is quitting.
pub fn wait_for_open() -> bool {
    let mut message = MSG::default();
    while unsafe { GetMessageW(&mut message, None, 0, 0) }.0 > 0 {
        if message.hwnd.0.is_null() && message.message == WM_OPEN {
            return true;
        }
        if message.hwnd.0.is_null() && message.message == WM_EXIT {
            return false;
        }
        if message.hwnd.0.is_null() && message.message == WM_SYNC {
            let _ = sync(true, &rules::load());
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
    false
}

/// Menu and click events arrive on the UI thread whether or not the window is open, so they
/// are handled here instead of in `update`.
pub fn install_handlers(shared: Arc<Shared>) {
    let menu_shared = shared.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        OPEN => menu_shared.show_window(),
        PAUSE => toggle_pause(&menu_shared),
        FRONT => frost_last_app(&menu_shared),
        EXIT => request_exit(&menu_shared),
        id => {
            if let Some(process) = id.strip_prefix(RULE) {
                toggle_rule(&menu_shared, process);
            }
        }
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            shared.show_window();
        }
    }));
}

/// Create or remove the icon to match the setting, and keep the menu in step with the rules.
pub fn sync(enabled: bool, store: &Store) -> Result<(), String> {
    TRAY.with(|slot| {
        let Ok(mut slot) = slot.try_borrow_mut() else {
            return Ok(());
        };
        if !enabled {
            *slot = None;
            return Ok(());
        }
        let apps: Vec<(String, bool)> = store
            .rules
            .iter()
            .map(|rule| (rule.process.clone(), rule.enabled))
            .collect();
        match slot.as_mut() {
            None => *slot = Some(build(store.paused, apps)?),
            Some(tray) if tray.apps != apps => {
                let (menu, pause) = menu(store.paused, &apps)?;
                tray.icon.set_menu(Some(Box::new(menu)));
                tray.pause = pause;
                tray.apps = apps;
            }
            Some(_) => {}
        }
        if let Some(tray) = slot.as_ref() {
            if tray.pause.is_checked() != store.paused {
                tray.pause.set_checked(store.paused);
            }
            let _ = tray.icon.set_tooltip(Some(tooltip(store)));
        }
        Ok(())
    })
}

fn tooltip(store: &Store) -> String {
    let frosted = store.rules.iter().filter(|rule| rule.enabled).count();
    match (store.paused, frosted) {
        (true, _) => "Blurman: paused".to_string(),
        (false, 0) => "Blurman".to_string(),
        (false, 1) => "Blurman: 1 app frosted".to_string(),
        (false, many) => format!("Blurman: {many} apps frosted"),
    }
}

fn menu(paused: bool, apps: &[(String, bool)]) -> Result<(Menu, CheckMenuItem), String> {
    let open = MenuItem::with_id(OPEN, "Open Blurman", true, None);
    let pause = CheckMenuItem::with_id(PAUSE, "Pause", true, paused, None);
    let front = MenuItem::with_id(FRONT, "Frost or unfrost the last app", true, None);
    let exit = MenuItem::with_id(EXIT, "Exit", true, None);
    let menu = Menu::new();
    let err = |err: tray_icon::menu::Error| err.to_string();
    menu.append_items(&[&open, &pause, &front]).map_err(err)?;
    if !apps.is_empty() {
        let submenu = Submenu::new("Apps", true);
        for (process, enabled) in apps {
            let id = format!("{RULE}{process}");
            submenu
                .append(&CheckMenuItem::with_id(id, process, true, *enabled, None))
                .map_err(err)?;
        }
        menu.append(&submenu).map_err(err)?;
    }
    menu.append_items(&[&PredefinedMenuItem::separator(), &exit])
        .map_err(err)?;
    Ok((menu, pause))
}

fn build(paused: bool, apps: Vec<(String, bool)>) -> Result<Tray, String> {
    let (menu, pause) = menu(paused, &apps)?;
    let icon = crate::app::tray_icon();
    let icon = tray_icon::Icon::from_rgba(icon.rgba, icon.width, icon.height)
        .map_err(|err| err.to_string())?;
    let icon = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_menu_on_left_click(false)
        .with_tooltip("Blurman")
        .with_icon(icon)
        .build()
        .map_err(|err| format!("Could not add the tray icon: {err}"))?;
    Ok(Tray { icon, pause, apps })
}

/// Save a change made from the menu and tell the effect thread. Its reload asks for the menu to
/// be brought up to date, after this handler has returned.
fn save_store(shared: &Shared, store: &Store) {
    if let Err(err) = rules::save(store) {
        shared.set_status(format!("Could not save rules: {err}"));
        return;
    }
    if !ipc::signal(ipc::msg_reload()) {
        shared.sync_tray();
    }
}

fn toggle_pause(shared: &Shared) {
    let mut store = rules::load();
    store.paused = !store.paused;
    save_store(shared, &store);
}

fn toggle_rule(shared: &Shared, process: &str) {
    let mut store = rules::load();
    if let Some(rule) = store.rule_mut(process) {
        rule.enabled = !rule.enabled;
        save_store(shared, &store);
    }
}

/// Opening the menu takes focus from the app you were using, so this acts on the app the effect
/// thread saw last instead of the window in front.
fn frost_last_app(shared: &Shared) {
    let hwnd = target::hwnd_of(shared.last_app.load(Ordering::SeqCst));
    if hwnd.0.is_null() || !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
        return;
    }
    let Some(process) = target::process_name(target::pid_of(hwnd)) else {
        return;
    };
    let mut store = rules::load();
    store.toggle_app(&process, Look::default());
    save_store(shared, &store);
}

/// Quit for real: the window closes without going to the tray, and Blurman then puts every app
/// back and exits normally.
fn request_exit(shared: &Shared) {
    shared.exiting.store(true, Ordering::SeqCst);
    let window = target::hwnd_of(shared.main_window.load(Ordering::SeqCst));
    unsafe {
        if !window.0.is_null() {
            let _ = PostMessageW(Some(window), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
        let _ = PostThreadMessageW(
            shared.ui_thread.load(Ordering::SeqCst),
            WM_EXIT,
            WPARAM(0),
            LPARAM(0),
        );
    }
}

/// Take the icon down. Called once the window and the tray loop are done, outside any handler.
pub fn remove() {
    TRAY.with(|slot| {
        if let Ok(mut slot) = slot.try_borrow_mut() {
            slot.take();
        }
    });
}
