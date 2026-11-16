//! Cloaking a window together with its glass. A cloaked window is left out by the compositor
//! and clicks pass it by, but it stays shown as far as Windows is concerned, so it keeps getting
//! paint messages. That matters for egui: a frame asked for while the window is hidden never
//! comes, and the window library then waits for it on a whole core.
//!
//! The served window keeps its backdrop's handle in a window property, so this works from any
//! thread without the panel itself.

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmSetWindowAttribute, DWMWA_CLOAK, DWMWA_CLOAKED,
};
use windows::Win32::UI::WindowsAndMessaging::{GetPropW, RemovePropW, SetPropW};

/// The property on the served window that holds its backdrop window.
const BACKDROP: PCWSTR = w!("EspalierGlassBackdrop");

/// DWM_CLOAKED_APP: cloaked by its own process (the other reasons are Windows' own).
const BY_APP: u32 = 1;

fn set(hwnd: HWND, on: bool) {
    let value = u32::from(on);
    unsafe {
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_CLOAK, (&raw const value).cast(), 4);
    }
}

/// Cloak or uncloak `hwnd` and its glass, from any thread. On the way in the glass goes first,
/// on the way out the content does, so neither is ever seen without the other behind it.
pub fn cloak(hwnd: isize, on: bool) {
    let target = HWND(hwnd as *mut _);
    let backdrop = backdrop_of(target);
    if on {
        set(target, true);
        if let Some(b) = backdrop {
            set(b, true);
        }
    } else {
        if let Some(b) = backdrop {
            set(b, false);
        }
        set(target, false);
    }
}

/// Whether `hwnd` is cloaked by its own process.
pub fn is_cloaked(hwnd: isize) -> bool {
    let mut value = 0u32;
    unsafe {
        let _ = DwmGetWindowAttribute(
            HWND(hwnd as *mut _),
            DWMWA_CLOAKED,
            (&raw mut value).cast(),
            4,
        );
    }
    value & BY_APP != 0
}

/// Record `backdrop` as the glass of `target`, cloaked as `target` is now.
pub(super) fn remember(target: HWND, backdrop: HWND) {
    unsafe {
        let _ = SetPropW(target, BACKDROP, Some(HANDLE(backdrop.0)));
    }
    set(backdrop, is_cloaked(target.0 as isize));
}

pub(super) fn forget(target: HWND) {
    unsafe {
        let _ = RemovePropW(target, BACKDROP);
    }
}

fn backdrop_of(target: HWND) -> Option<HWND> {
    let handle = unsafe { GetPropW(target, BACKDROP) };
    (!handle.is_invalid()).then_some(HWND(handle.0))
}
