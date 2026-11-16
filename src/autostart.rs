//! Opening Blurman when you sign in to Windows: the per-user Run entry for the portable exe, or
//! the package's startup task for the Microsoft Store version.

use crate::package;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ,
    RRF_RT_REG_BINARY, RRF_RT_REG_SZ,
};

pub const STARTUP_FLAG: &str = "--startup";

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
/// Where Task Manager and Settings note a startup app you switched off there. The Run entry
/// stays as it is.
const APPROVED_KEY: PCWSTR =
    w!("Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\StartupApproved\\Run");

/// "Blurman", or "Blurman-name" under a profile, so a test build never repoints your own entry.
fn value_name() -> HSTRING {
    HSTRING::from(crate::ipc::scoped("Blurman"))
}

pub fn is_enabled() -> bool {
    if package::is_packaged() {
        return package::startup_enabled();
    }
    read().is_some() && !switched_off_in_windows()
}

/// Whether Task Manager or Settings has this startup entry switched off.
fn switched_off_in_windows() -> bool {
    let mut bytes = [0u8; 32];
    let mut size = bytes.len() as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            APPROVED_KEY,
            &value_name(),
            RRF_RT_REG_BINARY,
            None,
            Some(bytes.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    err == ERROR_SUCCESS && approval_is_off(&bytes[..(size as usize).min(bytes.len())])
}

/// The note is a few bytes, and an odd first byte means off: 02 is on, 03 is off.
fn approval_is_off(bytes: &[u8]) -> bool {
    bytes.first().is_some_and(|first| first & 1 == 1)
}

/// Forget a switch-off made in Task Manager or Settings. Only the switch in Blurman does this:
/// it is you saying what you want now.
fn clear_approval() {
    unsafe {
        let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, APPROVED_KEY, &value_name());
    }
}

pub fn set_enabled(on: bool) -> Result<(), String> {
    if package::is_packaged() {
        return package::set_startup(on);
    }
    if !on {
        let err = unsafe { RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, &value_name()) };
        return if err == ERROR_SUCCESS || err == ERROR_FILE_NOT_FOUND {
            clear_approval();
            Ok(())
        } else {
            Err(format!(
                "Could not turn off start with Windows ({}).",
                err.0
            ))
        };
    }
    write_entry()?;
    clear_approval();
    Ok(())
}

/// Write the Run entry for this copy of Blurman. A switch-off made in Windows is left alone.
fn write_entry() -> Result<(), String> {
    let wide: Vec<u16> = command()?
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let err = unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            &value_name(),
            REG_SZ.0,
            Some(wide.as_ptr() as *const std::ffi::c_void),
            (wide.len() * 2) as u32,
        )
    };
    if err == ERROR_SUCCESS {
        Ok(())
    } else {
        Err(format!("Could not turn on start with Windows ({}).", err.0))
    }
}

/// Point an existing entry at this copy of Blurman, in case the exe was moved.
pub fn refresh() {
    // The package's startup task always points at the installed app.
    if package::is_packaged() {
        return;
    }
    if let (Some(current), Ok(wanted)) = (read(), command()) {
        if current != wanted {
            let _ = write_entry();
        }
    }
}

fn command() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|err| err.to_string())?;
    Ok(match crate::ipc::profile() {
        Some(profile) => format!("\"{}\" {STARTUP_FLAG} --profile {profile}", exe.display()),
        None => format!("\"{}\" {STARTUP_FLAG}", exe.display()),
    })
}

fn read() -> Option<String> {
    let mut size = 0u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            &value_name(),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut size),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u16; size as usize / 2 + 1];
    let mut size = (buf.len() * 2) as u32;
    let err = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            &value_name(),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    if err != ERROR_SUCCESS {
        return None;
    }
    let len = buf.iter().position(|ch| *ch == 0).unwrap_or(buf.len());
    Some(String::from_utf16_lossy(&buf[..len]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_startup_entry_switched_off_in_windows_reads_as_off() {
        // As Windows writes them.
        assert!(!approval_is_off(&[2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(approval_is_off(&[3, 0, 0, 0, 0x5c, 0x11, 0, 0, 0, 0, 0, 0]));
        assert!(!approval_is_off(&[6, 0, 0, 0]));
        assert!(approval_is_off(&[7, 0, 0, 0]));
        // No note at all: nothing was switched off.
        assert!(!approval_is_off(&[]));
    }
}
