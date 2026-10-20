//! What Windows is doing right now: theme colors, battery saver, Do not disturb, the clock.

use crate::mapping::Rgb;
use windows::core::{s, w, HSTRING};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::Shell::{
    SHQueryUserNotificationState, ShellExecuteW, QUNS_PRESENTATION_MODE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetForegroundWindow, GA_ROOT, SW_SHOWNORMAL,
};
use windows::UI::ViewManagement::{UIColorType, UISettings};

/// The colors a Theme or Accent tint follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SystemColors {
    pub dark: bool,
    pub accent: Rgb,
}

impl Default for SystemColors {
    fn default() -> Self {
        Self {
            dark: true,
            accent: Rgb(0x00, 0x78, 0xd4),
        }
    }
}

impl SystemColors {
    /// The Windows 11 window backgrounds, dark and light.
    pub fn theme_tint(&self) -> Rgb {
        if self.dark {
            Rgb(0x20, 0x20, 0x20)
        } else {
            Rgb(0xf3, 0xf3, 0xf3)
        }
    }
}

pub fn system_colors() -> SystemColors {
    let Ok(settings) = UISettings::new() else {
        return SystemColors::default();
    };
    let color = |kind| settings.GetColorValue(kind).ok();
    let mut colors = SystemColors::default();
    // Apps in dark mode get a black background color, light mode a white one.
    if let Some(background) = color(UIColorType::Background) {
        let light = background.R as u32 + background.G as u32 + background.B as u32;
        colors.dark = light < 384;
    }
    if let Some(accent) = color(UIColorType::Accent) {
        colors.accent = Rgb(accent.R, accent.G, accent.B);
    }
    colors
}

/// Battery saver is on, whether Windows turned it on or you did.
pub fn battery_saver() -> bool {
    let mut status = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut status) }.is_ok() && status.SystemStatusFlag == 1
}

/// Why notifications are being held back, if they are: Do not disturb or presentation mode.
pub fn quiet_reason() -> Option<&'static str> {
    if unsafe { SHQueryUserNotificationState() } == Ok(QUNS_PRESENTATION_MODE) {
        return Some("presentation mode is on");
    }
    focus_assist_on().then_some("Do not disturb is on")
}

/// The shell publishes the Focus assist and Do not disturb profile through a WNF state, and
/// nothing documented reports it. Zero is off; any other value holds notifications back.
fn focus_assist_on() -> bool {
    type QueryFn = unsafe extern "system" fn(
        *const u64,
        *const std::ffi::c_void,
        *const std::ffi::c_void,
        *mut u32,
        *mut std::ffi::c_void,
        *mut u32,
    ) -> i32;
    static QUERY: std::sync::OnceLock<Option<QueryFn>> = std::sync::OnceLock::new();
    const WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED: u64 = 0x0d83_063e_a3bf_1c75;
    let query = *QUERY.get_or_init(|| unsafe {
        let ntdll = LoadLibraryW(w!("ntdll.dll")).ok()?;
        let proc = GetProcAddress(ntdll, s!("NtQueryWnfStateData"))?;
        Some(std::mem::transmute::<
            unsafe extern "system" fn() -> isize,
            QueryFn,
        >(proc))
    });
    let Some(query) = query else {
        return false;
    };
    let mut stamp = 0u32;
    let mut profile = 0u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        query(
            &WNF_SHEL_QUIETHOURS_ACTIVE_PROFILE_CHANGED,
            std::ptr::null(),
            std::ptr::null(),
            &mut stamp,
            (&raw mut profile).cast(),
            &mut size,
        )
    };
    status >= 0 && size >= 4 && profile != 0
}

/// Local time as minutes after midnight.
pub fn local_minutes() -> u16 {
    let now = unsafe { GetLocalTime() };
    now.wHour * 60 + now.wMinute
}

/// The top-level window you are using, or None while the desktop or nothing has focus.
pub fn foreground_root() -> Option<HWND> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return None;
    }
    let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
    Some(if root.0.is_null() { hwnd } else { root })
}

/// Open a web page or a folder in the default handler.
pub fn open(target: &str) {
    unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            &HSTRING::from(target),
            None,
            None,
            SW_SHOWNORMAL,
        );
    }
}
