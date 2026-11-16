//! The backdrop window: a borderless popup that only shows a composition tree, sits right under
//! the window it serves, and is invisible to the mouse, the keyboard, the taskbar and Alt+Tab.

use std::sync::Once;
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWINDOWATTRIBUTE};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, RegisterClassW, SetLayeredWindowAttributes, LWA_ALPHA,
    WM_DPICHANGED, WM_MOUSEACTIVATE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT, WS_POPUP,
};

const CLASS: windows::core::PCWSTR = w!("EspalierGlass");

/// Lets the window's composition tree use the host backdrop brush (the blurred desktop behind
/// it). `DWMWA_USE_HOSTBACKDROPBRUSH`, which this version of the `windows` crate does not name.
const USE_HOST_BACKDROP: DWMWINDOWATTRIBUTE = DWMWINDOWATTRIBUTE(17);

/// A hidden backdrop window, or None if Windows would not make one.
///
/// It is layered and transparent to the mouse, so clicks on it (the shadow, or the corners
/// outside the shape) reach whatever is behind, in any process. It has no redirection bitmap: all
/// it shows is its composition tree.
pub fn create() -> Option<HWND> {
    static REGISTER: Once = Once::new();
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        REGISTER.call_once(|| {
            let class = WNDCLASSW {
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: CLASS,
                ..Default::default()
            };
            RegisterClassW(&class);
        });
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP
                | WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOOLWINDOW
                | WS_EX_NOACTIVATE,
            CLASS,
            w!("blurman glass"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        // A layered window shows nothing until it is given an opacity.
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
        let on: u32 = 1;
        let _ = DwmSetWindowAttribute(hwnd, USE_HOST_BACKDROP, (&raw const on).cast(), 4);
        Some(hwnd)
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // HTTRANSPARENT, for the rare caller that asks directly.
        WM_NCHITTEST => LRESULT(-1),
        // MA_NOACTIVATE.
        WM_MOUSEACTIVATE => LRESULT(3),
        // Its size always comes from the window it serves, measured at that window's DPI, so the
        // size Windows suggests here is ignored.
        WM_DPICHANGED => LRESULT(0),
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
