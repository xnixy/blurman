//! Running from the Microsoft Store package rather than as the portable exe.
//!
//! The same exe serves both. Inside the package, Windows starts Blurman at sign-in through the
//! startup task the manifest declares instead of the registry, the Store delivers updates, and
//! files Blurman writes under %APPDATA% are kept in the package's own folder.

use std::sync::OnceLock;
use windows::core::{HSTRING, PWSTR};
use windows::ApplicationModel::Activation::ActivationKind;
use windows::ApplicationModel::{AppInstance, StartupTask, StartupTaskState};
use windows::Win32::Foundation::{APPMODEL_ERROR_NO_PACKAGE, ERROR_SUCCESS};
use windows::Win32::Storage::Packaging::Appx::{
    GetCurrentPackageFamilyName, GetCurrentPackageFullName,
};
use windows::Win32::System::Com::CoIncrementMTAUsage;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

/// Blurman's page in the Microsoft Store, where the signed version is.
pub const STORE_URL: &str = "https://apps.microsoft.com/detail/9NQ3GQNBM4LK";

/// The startup task's id in packaging/msix/AppxManifest.xml.
const STARTUP_TASK: &str = "BlurmanStartup";

/// Whether this Blurman runs from an installed package.
pub fn is_packaged() -> bool {
    static PACKAGED: OnceLock<bool> = OnceLock::new();
    *PACKAGED.get_or_init(|| {
        let mut length = 0u32;
        let result = unsafe { GetCurrentPackageFullName(&mut length, None) };
        result != APPMODEL_ERROR_NO_PACKAGE
    })
}

/// The package family name, like "xnixy.Blurman_nz2sgw67tcfww". None outside a package.
pub fn family_name() -> Option<String> {
    if !is_packaged() {
        return None;
    }
    let mut buffer = [0u16; 256];
    let mut length = buffer.len() as u32;
    let result =
        unsafe { GetCurrentPackageFamilyName(&mut length, Some(PWSTR(buffer.as_mut_ptr()))) };
    (result == ERROR_SUCCESS)
        .then(|| String::from_utf16_lossy(&buffer[..length.saturating_sub(1) as usize]))
}

/// Run a Windows Runtime call on its own thread, set up for it. Startup task calls wait on
/// Windows, and doing that on the window's thread could stall it.
fn on_winrt_thread<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    // Without this, the shared apartment closes when the first helper thread ends, while
    // windows-rs keeps the startup task's factory cached. The next call used that dead factory
    // and crashed in combase.dll, so switching Start with Windows took Blurman down.
    static MTA: OnceLock<()> = OnceLock::new();
    MTA.get_or_init(|| {
        let _ = unsafe { CoIncrementMTAUsage() };
    });
    std::thread::spawn(move || {
        let _ = unsafe { RoInitialize(RO_INIT_MULTITHREADED) };
        call()
    })
    .join()
    .ok()
}

/// Windows started this Blurman at sign-in, through the package's startup task.
pub fn launched_at_startup() -> bool {
    is_packaged()
        && on_winrt_thread(|| {
            AppInstance::GetActivatedEventArgs()
                .and_then(|args| args.Kind())
                .is_ok_and(|kind| kind == ActivationKind::StartupTask)
        })
        .unwrap_or(false)
}

fn startup_task() -> windows::core::Result<StartupTask> {
    StartupTask::GetAsync(&HSTRING::from(STARTUP_TASK))?.get()
}

pub fn startup_enabled() -> bool {
    on_winrt_thread(|| {
        startup_task()
            .and_then(|task| task.State())
            .is_ok_and(|state| {
                state == StartupTaskState::Enabled || state == StartupTaskState::EnabledByPolicy
            })
    })
    .unwrap_or(false)
}

pub fn set_startup(on: bool) -> Result<(), String> {
    on_winrt_thread(move || {
        let task = startup_task().map_err(|err| format!("Could not reach the startup task: {err}"))?;
        if !on {
            return task.Disable().map_err(|err| err.to_string());
        }
        let state = task
            .RequestEnableAsync()
            .and_then(|request| request.get())
            .map_err(|err| err.to_string())?;
        match state {
            StartupTaskState::Enabled | StartupTaskState::EnabledByPolicy => Ok(()),
            StartupTaskState::DisabledByUser => Err(
                "Windows has Blurman switched off as a startup app. Turn it on in Settings, Apps, Startup."
                    .into(),
            ),
            _ => Err("Your organization does not allow startup apps.".into()),
        }
    })
    .unwrap_or_else(|| Err("Could not reach the startup task.".into()))
}
