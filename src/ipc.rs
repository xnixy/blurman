//! Find the running Blurman and tell it the rules file changed.

use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{
    CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND, LPARAM, WPARAM,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, FindWindowW, PostMessageW, RegisterWindowMessageW, ASFW_ANY,
};

const DETACHED_PROCESS: u32 = 0x0000_0008;

/// Set BLURMAN_PROFILE to run a separate Blurman beside the usual one, with its own files,
/// instance lock, and host window. Used to try a build without touching your own rules.
pub fn profile() -> Option<&'static str> {
    static PROFILE: OnceLock<Option<String>> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            std::env::var("BLURMAN_PROFILE").ok().filter(|name| {
                !name.is_empty() && name.chars().all(|ch| ch.is_ascii_alphanumeric())
            })
        })
        .as_deref()
}

/// `base`, or `base-profile` while a profile is set.
pub fn scoped(base: &str) -> String {
    match profile() {
        Some(profile) => format!("{base}-{profile}"),
        None => base.to_string(),
    }
}

/// Class and title of the effect thread's hidden window, which other launches look for.
pub fn host_class() -> PCWSTR {
    static CLASS: OnceLock<HSTRING> = OnceLock::new();
    PCWSTR(
        CLASS
            .get_or_init(|| HSTRING::from(scoped("BlurmanHost")))
            .as_ptr(),
    )
}

pub fn msg_reload() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Reload")) }
}

pub fn msg_show() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Show")) }
}

pub fn msg_shutdown() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Shutdown")) }
}

pub fn msg_tweak() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Tweak")) }
}

/// settings.json changed: shortcuts and auto-pause are read again.
pub fn msg_settings() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Settings")) }
}

/// Windows changed its theme, accent, or power state.
pub fn msg_environment() -> u32 {
    unsafe { RegisterWindowMessageW(windows::core::w!("Blurman.Environment")) }
}

pub fn find_host() -> Option<HWND> {
    unsafe { FindWindowW(host_class(), host_class()) }
        .ok()
        .filter(|hwnd| !hwnd.0.is_null())
}

pub fn signal(message: u32) -> bool {
    let Some(hwnd) = find_host() else {
        return false;
    };
    unsafe { PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0)).is_ok() }
}

/// Bring the running Blurman window forward. Returns false if none is running.
pub fn show_running() -> bool {
    unsafe {
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
    signal(msg_show())
}

/// Held for the life of the Blurman that owns the window and the glass.
pub struct Instance(HANDLE);

impl Drop for Instance {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.0);
        }
    }
}

/// Become the one running Blurman in this session, or None if another already is.
pub fn claim_instance() -> Option<Instance> {
    unsafe {
        let name = HSTRING::from(scoped("Local\\Blurman.Instance"));
        let handle = CreateMutexW(None, false, &name).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let _ = CloseHandle(handle);
            return None;
        }
        Some(Instance(handle))
    }
}

/// Bring the other Blurman forward. It may still be starting, so wait for its host window.
pub fn show_when_ready(timeout: Duration) {
    let start = Instant::now();
    while !show_running() && start.elapsed() < timeout {
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Tell a running Blurman to reload the rules, or open Blurman if none is running.
pub fn reload_or_launch() -> Result<(), String> {
    if signal(msg_reload()) {
        return Ok(());
    }
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    Command::new(exe)
        .creation_flags(DETACHED_PROCESS)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not start Blurman: {err}"))
}
