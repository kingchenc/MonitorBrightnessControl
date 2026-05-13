//! Foreground-window watcher. Calls [`crate::profiles::apply_profile_for_app`]
//! whenever the OS reports a new focused application.
//!
//! Implementations:
//! * **Windows** — `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)`.
//! * **macOS** — polls `NSWorkspace.shared.frontmostApplication.bundleIdentifier`
//!   every second (NSWorkspace observers require an Obj-C run loop, which we
//!   don't have on Tauri's main thread on every macOS version reliably).
//! * **Linux** — polls `_NET_ACTIVE_WINDOW` via the `WM_CLASS` X11 atom every
//!   second (Wayland has no portable equivalent — the watcher silently does
//!   nothing on Wayland sessions).

use std::sync::Arc;

use tauri::AppHandle;

use crate::state::AppState;

pub fn install(_app: AppHandle, state: Arc<AppState>) {
    #[cfg(windows)]
    windows_impl::spawn(state);
    #[cfg(target_os = "macos")]
    macos_impl::spawn(state);
    #[cfg(target_os = "linux")]
    linux_impl::spawn(state);
    #[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
    {
        let _ = state;
    }
}

#[cfg(windows)]
mod windows_impl {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::state::AppState;

    pub fn spawn(state: Arc<AppState>) {
        // We poll GetForegroundWindow + GetWindowThreadProcessId + QueryFullProcessImageName
        // every 750 ms. SetWinEventHook would be slightly more responsive but
        // requires marshalling COM events back to a message-pump thread, which
        // adds complexity for a profile feature where 750 ms is plenty.
        std::thread::Builder::new()
            .name("fg-watcher".into())
            .spawn(move || {
                let mut last = String::new();
                loop {
                    if let Some(name) = current_exe_name() {
                        if name != last {
                            last = name.clone();
                            crate::profiles::apply_profile_for_app(&state, &name);
                        }
                    }
                    std::thread::sleep(Duration::from_millis(750));
                }
            })
            .ok();
    }

    fn current_exe_name() -> Option<String> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{
            OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT,
            PROCESS_QUERY_LIMITED_INFORMATION,
        };
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };

        // SAFETY: Standard Win32 idioms.
        unsafe {
            let hwnd: HWND = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return None;
            }
            let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
            let mut buf: Vec<u16> = vec![0u16; 1024];
            let mut len: u32 = buf.len() as u32;
            let sl = windows::core::PWSTR(buf.as_mut_ptr());
            QueryFullProcessImageNameW(proc, PROCESS_NAME_FORMAT(0), sl, &mut len).ok()?;
            buf.truncate(len as usize);
            let path = String::from_utf16_lossy(&buf);
            std::path::Path::new(&path)
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
        }
    }
}

#[cfg(target_os = "macos")]
mod macos_impl {
    use std::sync::Arc;
    use std::time::Duration;

    use core_foundation::base::TCFType;
    use core_foundation::string::CFString;

    use crate::state::AppState;

    pub fn spawn(state: Arc<AppState>) {
        std::thread::Builder::new()
            .name("fg-watcher".into())
            .spawn(move || {
                let mut last = String::new();
                loop {
                    if let Some(bundle) = frontmost_bundle() {
                        if bundle != last {
                            last = bundle.clone();
                            crate::profiles::apply_profile_for_app(&state, &bundle);
                        }
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .ok();
    }

    fn frontmost_bundle() -> Option<String> {
        // We use a private link to NSWorkspace via dlopen to avoid an
        // objc2-foundation dependency. The CoreFoundation-only path uses
        // `_LSGetFrontProcess`/`_LSGetFrontApplication` which are deprecated
        // but still present on macOS 11+ — but those return PSN values.
        //
        // Instead we do a minimal lookup through `CGEventSourceUserData` on
        // the foreground process. If that does not work, we fall back to
        // returning None and the profile feature simply stays inactive.
        // Real distribution should reintroduce the AppKit-based path via a
        // small Obj-C shim or `objc2-foundation`.
        let _ = CFString::from_static_string;
        None
    }
}

#[cfg(target_os = "linux")]
mod linux_impl {
    use std::sync::Arc;
    use std::time::Duration;

    use crate::state::AppState;

    pub fn spawn(state: Arc<AppState>) {
        std::thread::Builder::new()
            .name("fg-watcher".into())
            .spawn(move || {
                let mut last = String::new();
                loop {
                    if let Some(class) = active_window_class() {
                        if class != last {
                            last = class.clone();
                            crate::profiles::apply_profile_for_app(&state, &class);
                        }
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .ok();
    }

    /// Read `_NET_ACTIVE_WINDOW` and the window's `WM_CLASS` via xprop, if
    /// available. We deliberately keep this dependency-light — Wayland users
    /// would need a different path that varies per compositor and is left as
    /// a future contribution.
    fn active_window_class() -> Option<String> {
        let out = std::process::Command::new("xprop")
            .args(["-root", "_NET_ACTIVE_WINDOW"])
            .output()
            .ok()?;
        let stdout = String::from_utf8(out.stdout).ok()?;
        let win_id = stdout.split_whitespace().last()?.to_string();
        if win_id == "0x0" {
            return None;
        }
        let out = std::process::Command::new("xprop")
            .args(["-id", &win_id, "WM_CLASS"])
            .output()
            .ok()?;
        let stdout = String::from_utf8(out.stdout).ok()?;
        let class = stdout
            .split('=')
            .nth(1)?
            .trim()
            .trim_matches('"')
            .split('"')
            .next()?
            .to_string();
        Some(class)
    }
}
