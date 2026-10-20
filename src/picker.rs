//! Click a window to pick its app. A clear overlay covers every monitor, the window under the
//! cursor is tinted rose, and a click hands its process name to the Blurman window.

use crate::shared::Shared;
use crate::target;
use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::CreateSolidBrush;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetMessageW,
    GetSystemMetrics, LoadCursorW, PostQuitMessage, RegisterClassExW, SetCursor,
    SetForegroundWindow, SetLayeredWindowAttributes, SetWindowPos, ShowWindow, TranslateMessage,
    IDC_CROSS, LWA_ALPHA, MSG, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SWP_NOACTIVATE, SWP_SHOWWINDOW, SW_HIDE, SW_SHOW, WM_KEYDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_RBUTTONUP, WM_SETCURSOR, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

const OVERLAY_CLASS: PCWSTR = w!("BlurmanPicker");
const FRAME_CLASS: PCWSTR = w!("BlurmanPickerFrame");
/// The rose accent, as a COLORREF (0x00BBGGRR).
const FRAME_COLOR: u32 = 0x004e_36c4;
const FRAME_ALPHA: u8 = 90;
/// Lets the Blurman window finish minimizing before the overlay takes focus.
const MINIMIZE_WAIT: Duration = Duration::from_millis(250);

#[derive(Default)]
struct Pick {
    overlay: HWND,
    frame: HWND,
    hovered: Option<String>,
    picked: Option<String>,
}

thread_local! {
    static PICK: RefCell<Pick> = RefCell::new(Pick::default());
}

/// Let the user click a window. The answer lands in `shared.picked`, and the Blurman window is
/// brought back either way.
pub fn start(shared: Arc<Shared>) {
    let picker_shared = shared.clone();
    let spawned = std::thread::Builder::new()
        .name("blurman-picker".into())
        .spawn(move || {
            std::thread::sleep(MINIMIZE_WAIT);
            match run() {
                Ok(picked) => picker_shared.set_picked(picked),
                Err(err) => picker_shared.set_status(err),
            }
            picker_shared.show_window();
        });
    if spawned.is_err() {
        // Without the thread nothing would bring the window back.
        shared.set_status("Could not start the window picker.");
        shared.show_window();
    }
}

fn run() -> Result<Option<String>, String> {
    let instance = unsafe { GetModuleHandleW(None) }.map_err(|err| err.to_string())?;
    let instance = HINSTANCE(instance.0);
    register(instance)?;
    let (x, y, width, height) = unsafe {
        (
            GetSystemMetrics(SM_XVIRTUALSCREEN),
            GetSystemMetrics(SM_YVIRTUALSCREEN),
            GetSystemMetrics(SM_CXVIRTUALSCREEN),
            GetSystemMetrics(SM_CYVIRTUALSCREEN),
        )
    };
    let create = |class: PCWSTR, exstyle| unsafe {
        CreateWindowExW(
            exstyle,
            class,
            class,
            WS_POPUP,
            x,
            y,
            width,
            height,
            None,
            None,
            Some(instance),
            None,
        )
        .map_err(|err| format!("Could not start the window picker: {err}"))
    };
    let frame = create(
        FRAME_CLASS,
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE,
    )?;
    let overlay = match create(
        OVERLAY_CLASS,
        WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
    ) {
        Ok(overlay) => overlay,
        Err(err) => {
            unsafe {
                let _ = DestroyWindow(frame);
            }
            return Err(err);
        }
    };
    PICK.with(|pick| {
        *pick.borrow_mut() = Pick {
            overlay,
            frame,
            ..Pick::default()
        }
    });
    unsafe {
        // Nearly clear, but not fully: a window with alpha 0 would let clicks through.
        let _ = SetLayeredWindowAttributes(overlay, COLORREF(0), 1, LWA_ALPHA);
        let _ = SetLayeredWindowAttributes(frame, COLORREF(0), FRAME_ALPHA, LWA_ALPHA);
        let _ = ShowWindow(overlay, SW_SHOW);
        let _ = SetForegroundWindow(overlay);
        let _ = SetFocus(Some(overlay));
        let mut cursor = POINT::default();
        if GetCursorPos(&mut cursor).is_ok() {
            hover(cursor);
        }
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).0 > 0 {
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        let _ = DestroyWindow(overlay);
        let _ = DestroyWindow(frame);
    }
    Ok(PICK.with(|pick| pick.borrow_mut().picked.take()))
}

fn register(instance: HINSTANCE) -> Result<(), String> {
    static RESULT: std::sync::OnceLock<Result<(), String>> = std::sync::OnceLock::new();
    RESULT
        .get_or_init(|| unsafe {
            let overlay = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance,
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                lpszClassName: OVERLAY_CLASS,
                ..Default::default()
            };
            let frame = WNDCLASSEXW {
                cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(frame_proc),
                hInstance: instance,
                hbrBackground: CreateSolidBrush(COLORREF(FRAME_COLOR)),
                lpszClassName: FRAME_CLASS,
                ..Default::default()
            };
            if RegisterClassExW(&overlay) == 0 || RegisterClassExW(&frame) == 0 {
                return Err("Could not register the window picker.".into());
            }
            Ok(())
        })
        .clone()
}

/// Tint the window under `point`, if it is one Blurman could frost.
fn hover(point: POINT) {
    PICK.with(|pick| {
        let mut pick = pick.borrow_mut();
        match target::frostable_at(point) {
            Some((_, process, bounds)) => {
                unsafe {
                    // Just under the overlay, so the overlay keeps the clicks.
                    let _ = SetWindowPos(
                        pick.frame,
                        Some(pick.overlay),
                        bounds.left,
                        bounds.top,
                        bounds.right - bounds.left,
                        bounds.bottom - bounds.top,
                        SWP_NOACTIVATE | SWP_SHOWWINDOW,
                    );
                }
                pick.hovered = Some(process);
            }
            None => {
                unsafe {
                    let _ = ShowWindow(pick.frame, SW_HIDE);
                }
                pick.hovered = None;
            }
        }
    });
}

fn finish(take_hovered: bool) {
    PICK.with(|pick| {
        let mut pick = pick.borrow_mut();
        pick.picked = if take_hovered {
            pick.hovered.take()
        } else {
            None
        };
    });
    unsafe { PostQuitMessage(0) };
}

unsafe extern "system" fn frame_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn overlay_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_MOUSEMOVE => {
            let mut cursor = POINT::default();
            if GetCursorPos(&mut cursor).is_ok() {
                hover(cursor);
            }
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let mut cursor = POINT::default();
            if GetCursorPos(&mut cursor).is_ok() {
                hover(cursor);
            }
            finish(true);
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            finish(false);
            LRESULT(0)
        }
        WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => {
            finish(false);
            LRESULT(0)
        }
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(None, IDC_CROSS).ok());
            LRESULT(1)
        }
        _ => DefWindowProcW(hwnd, msg, wparam, lparam),
    }
}
