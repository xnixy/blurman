//! Ask GitHub whether a newer Blurman is out. Nothing is downloaded: the window shows a link.

use crate::rules;
use crate::shared::{Shared, Update};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::core::{w, HSTRING};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryDataAvailable,
    WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts, INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
    WINHTTP_FLAG_SECURE, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

const HOST: &str = "api.github.com";
const LATEST: &str = "/repos/xnixy/blurman/releases/latest";
pub const RELEASES: &str = "https://github.com/xnixy/blurman/releases";
/// Give the window and the glass the first moments after start.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// A check that could not reach GitHub, as on a laptop opened with no network yet, is tried
/// again this much later, not a day later.
const RETRY: Duration = Duration::from_secs(60 * 60);

/// Check once a day, while "Check for updates" is on. Nothing is asked while it is off.
pub fn watch(shared: Arc<Shared>) {
    let _ = std::thread::Builder::new()
        .name("blurman-update".into())
        .spawn(move || {
            shared.set_update_watcher(std::thread::current());
            std::thread::sleep(FIRST_CHECK);
            let mut due = Instant::now();
            loop {
                let on = wanted();
                if on && Instant::now() >= due {
                    let result = check_once(&shared);
                    due = Instant::now() + wait_after(&result);
                    // The setting can be switched off while GitHub answers.
                    if let Some(Ok(Some(update))) = result {
                        if wanted() {
                            shared.set_update(Some(update));
                        }
                    }
                }
                // Switching the setting on wakes this. Until then there is nothing to do.
                let wait = if on {
                    due.saturating_duration_since(Instant::now())
                } else {
                    EVERY
                };
                std::thread::park_timeout(wait);
            }
        });
}

fn wanted() -> bool {
    rules::load_settings().check_updates
}

/// How long until the next daily check, after one that ended as `result`.
fn wait_after(result: &Option<Result<Option<Update>, String>>) -> Duration {
    match result {
        Some(Ok(_)) => EVERY,
        // It failed, or "Check now" was asking at that moment.
        Some(Err(_)) | None => RETRY,
    }
}

/// `check`, unless a check is already under way: then None, and nothing is asked twice.
pub fn check_once(shared: &Shared) -> Option<Result<Option<Update>, String>> {
    if shared.checking_update.swap(true, Ordering::SeqCst) {
        return None;
    }
    let result = check();
    shared.checking_update.store(false, Ordering::SeqCst);
    Some(result)
}

/// The latest release, if it is newer than this Blurman.
pub fn check() -> Result<Option<Update>, String> {
    let body = https_get(HOST, LATEST)?;
    let release: serde_json::Value = serde_json::from_str(&body).map_err(|err| err.to_string())?;
    let tag = release["tag_name"]
        .as_str()
        .ok_or("The release has no tag.")?;
    let url = release["html_url"].as_str().unwrap_or(RELEASES).to_string();
    let version = version_of(tag);
    Ok(newer(&version, env!("CARGO_PKG_VERSION")).then_some(Update { version, url }))
}

/// Release tags are "v0.6.0-1a2b3c4": the version, then the commit it was built from.
fn version_of(tag: &str) -> String {
    let tag = tag.trim().trim_start_matches('v');
    tag.split('-').next().unwrap_or(tag).to_string()
}

/// Whether dotted version `candidate` comes after `current`. Builds of the same version from
/// later commits do not count, so every push to main is not an update.
pub fn newer(candidate: &str, current: &str) -> bool {
    let parts = |text: &str| -> Vec<u64> {
        text.split('.')
            .map(|part| part.trim().parse().unwrap_or(0))
            .collect()
    };
    let (mut a, mut b) = (parts(candidate), parts(current));
    let len = a.len().max(b.len());
    a.resize(len, 0);
    b.resize(len, 0);
    a > b
}

/// A WinHTTP handle closed when dropped.
struct Handle(*mut std::ffi::c_void);

impl Handle {
    fn new(raw: *mut std::ffi::c_void, what: &str) -> Result<Self, String> {
        if raw.is_null() {
            Err(format!(
                "Could not {what}: {}",
                windows::core::Error::from_win32()
            ))
        } else {
            Ok(Self(raw))
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            let _ = WinHttpCloseHandle(self.0);
        }
    }
}

fn https_get(host: &str, path: &str) -> Result<String, String> {
    let agent = HSTRING::from(format!("Blurman/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = Handle::new(
            WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, None, None, 0),
            "start WinHTTP",
        )?;
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 15_000);
        let connection = Handle::new(
            WinHttpConnect(
                session.0,
                &HSTRING::from(host),
                INTERNET_DEFAULT_HTTPS_PORT,
                0,
            ),
            "reach GitHub",
        )?;
        let accept = [
            w!("application/vnd.github+json"),
            windows::core::PCWSTR::null(),
        ];
        let request = Handle::new(
            WinHttpOpenRequest(
                connection.0,
                w!("GET"),
                &HSTRING::from(path),
                None,
                None,
                accept.as_ptr(),
                WINHTTP_FLAG_SECURE,
            ),
            "ask GitHub",
        )?;
        WinHttpSendRequest(request.0, None, None, 0, 0, 0).map_err(|err| err.to_string())?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).map_err(|err| err.to_string())?;
        let mut status = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            None,
            Some((&raw mut status).cast()),
            &mut size,
            std::ptr::null_mut(),
        )
        .map_err(|err| err.to_string())?;
        if status != 200 {
            return Err(format!("GitHub answered {status}."));
        }
        let mut body = Vec::new();
        loop {
            let mut available = 0u32;
            WinHttpQueryDataAvailable(request.0, &mut available).map_err(|err| err.to_string())?;
            if available == 0 {
                break;
            }
            let start = body.len();
            body.resize(start + available as usize, 0);
            let mut read = 0u32;
            WinHttpReadData(
                request.0,
                body[start..].as_mut_ptr().cast(),
                available,
                &mut read,
            )
            .map_err(|err| err.to_string())?;
            body.truncate(start + read as usize);
            // GitHub's answer is a few kilobytes. Anything this large is not it.
            if body.len() > 4 << 20 {
                return Err("The answer from GitHub was too large.".into());
            }
        }
        String::from_utf8(body).map_err(|err| err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_check_is_tried_again_sooner_than_a_day() {
        assert_eq!(wait_after(&Some(Ok(None))), EVERY);
        assert_eq!(wait_after(&Some(Err("offline".into()))), RETRY);
        assert_eq!(wait_after(&None), RETRY);
        assert!(RETRY < EVERY);
    }

    #[test]
    fn two_checks_never_run_at_once() {
        let shared = Shared::new();
        shared.checking_update.store(true, Ordering::SeqCst);
        // Returns before asking anything: another check holds the flag.
        assert_eq!(check_once(&shared), None);
        assert!(shared.checking_update.load(Ordering::SeqCst));
    }

    #[test]
    fn only_a_higher_version_is_an_update() {
        assert!(newer("0.6.1", "0.6.0"));
        assert!(newer("1.0", "0.9.9"));
        assert!(newer("0.10.0", "0.9.0"));
        assert!(!newer("0.6.0", "0.6.0"));
        assert!(!newer("0.5.9", "0.6.0"));
        assert_eq!(version_of("v0.6.0-1a2b3c4"), "0.6.0");
        assert_eq!(version_of("0.7.0"), "0.7.0");
    }
}
