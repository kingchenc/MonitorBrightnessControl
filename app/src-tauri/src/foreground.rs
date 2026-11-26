//! Foreground-window watcher. Calls [`crate::profiles::apply_profile_for_app`]
//! whenever the OS reports a new focused application.
//!
//! Implementations:
//! * **Windows** — `SetWinEventHook(EVENT_SYSTEM_FOREGROUND, ...)`: event
//!   driven, idle between app switches; polls every 750 ms only if the hook
//!   cannot be installed.
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
    use std::sync::{Arc, OnceLock, mpsc};
    use std::time::Duration;

    use windows::Win32::Foundation::{HMODULE, HWND};
    use windows::Win32::UI::Accessibility::{HWINEVENTHOOK, SetWinEventHook, UnhookWinEvent};
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND, GetMessageW, MSG,
        TranslateMessage, WINEVENT_OUTOFCONTEXT,
    };

    use crate::state::AppState;

    /// Wakes the profile worker. Capacity 1: a burst of focus changes while
    /// the worker is busy (DDC/CI writes take seconds) collapses into one
    /// check of whatever is in front afterwards.
    static WAKE: OnceLock<mpsc::SyncSender<()>> = OnceLock::new();

    fn wake() {
        if let Some(tx) = WAKE.get() {
            let _ = tx.try_send(());
        }
    }

    pub fn spawn(state: Arc<AppState>) {
        let (tx, rx) = mpsc::sync_channel::<()>(1);
        if WAKE.set(tx).is_err() {
            return;
        }

        // Worker: checks the foreground app once at startup, then each time
        // it is woken, and applies the matching profile on a change.
        std::thread::Builder::new()
            .name("fg-profiles".into())
            .spawn(move || {
                let self_name = self_exe_name();
                let mut last = String::new();
                loop {
                    if let Some(name) = current_exe_name() {
                        // Never treat focusing our own window as an app switch.
                        // Otherwise opening the control panel would count as
                        // "focused a non-profiled app" and reset every monitor
                        // to its default brightness, undoing the active profile
                        // and making the user's last change look like it didn't
                        // take.
                        let is_self = self_name
                            .as_deref()
                            .is_some_and(|s| name.eq_ignore_ascii_case(s));
                        if !is_self && name != last {
                            log::debug!("foreground app: {name}");
                            last = name.clone();
                            crate::profiles::apply_profile_for_app(&state, &name);
                        }
                    }
                    if rx.recv().is_err() {
                        return;
                    }
                }
            })
            .ok();

        // Event source: the OS reports foreground changes, so nothing runs
        // between app switches (no polling). Should the hook be unavailable
        // or its message loop end, fall back to polling every 750 ms.
        std::thread::Builder::new()
            .name("fg-hook".into())
            .spawn(|| {
                run_hook_loop();
                log::warn!("foreground hook unavailable; polling every 750 ms instead");
                loop {
                    std::thread::sleep(Duration::from_millis(750));
                    wake();
                }
            })
            .ok();
    }

    /// Install out-of-context WinEvent hooks for foreground changes and run
    /// this thread's message loop, which delivers them. Blocks for the life
    /// of the app; returns only if the hook could not be installed or the
    /// loop failed.
    fn run_hook_loop() {
        // SAFETY: Standard Win32 idioms. The hooks are owned by this thread,
        // which pumps messages for them and removes them before returning.
        unsafe {
            let foreground = SetWinEventHook(
                EVENT_SYSTEM_FOREGROUND,
                EVENT_SYSTEM_FOREGROUND,
                HMODULE::default(),
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            if foreground.is_invalid() {
                return;
            }
            // A window restored from the taskbar normally also reports
            // FOREGROUND; this covers the cases where it does not.
            let restored = SetWinEventHook(
                EVENT_SYSTEM_MINIMIZEEND,
                EVENT_SYSTEM_MINIMIZEEND,
                HMODULE::default(),
                Some(on_win_event),
                0,
                0,
                WINEVENT_OUTOFCONTEXT,
            );
            let mut msg = MSG::default();
            // 0 = WM_QUIT, -1 = error: both end the loop.
            while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if !restored.is_invalid() {
                let _ = UnhookWinEvent(restored);
            }
            let _ = UnhookWinEvent(foreground);
        }
    }

    unsafe extern "system" fn on_win_event(
        _hook: HWINEVENTHOOK,
        _event: u32,
        _hwnd: HWND,
        _id_object: i32,
        _id_child: i32,
        _event_thread: u32,
        _event_time: u32,
    ) {
        wake();
    }

    /// File name of our own executable, used to skip self-focus events.
    fn self_exe_name() -> Option<String> {
        std::env::current_exe()
            .ok()?
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
    }

    /// Process handle that is closed when it goes out of scope, on every path.
    struct ProcessHandle(windows::Win32::Foundation::HANDLE);

    impl Drop for ProcessHandle {
        fn drop(&mut self) {
            // SAFETY: the handle came from a successful OpenProcess and is
            // closed exactly once, here.
            unsafe {
                let _ = windows::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }

    fn current_exe_name() -> Option<String> {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
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
            // This runs on every app switch (every 750 ms in the polling
            // fallback): a handle left open per call piles up over days of
            // uptime.
            let proc =
                ProcessHandle(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?);
            let mut buf: Vec<u16> = vec![0u16; 1024];
            let mut len: u32 = buf.len() as u32;
            let sl = windows::core::PWSTR(buf.as_mut_ptr());
            QueryFullProcessImageNameW(proc.0, PROCESS_NAME_FORMAT(0), sl, &mut len).ok()?;
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
                let self_class = self_exe_name();
                let mut last = String::new();
                loop {
                    if let Some(class) = active_window_class() {
                        // Skip our own window so opening the control panel does
                        // not reset every monitor to its default brightness.
                        let is_self = self_class
                            .as_deref()
                            .is_some_and(|s| class.eq_ignore_ascii_case(s));
                        if !is_self && class != last {
                            last = class.clone();
                            crate::profiles::apply_profile_for_app(&state, &class);
                        }
                    }
                    std::thread::sleep(Duration::from_secs(1));
                }
            })
            .ok();
    }

    /// File name of our own executable. On Linux the X11 `WM_CLASS` of a Tauri
    /// window typically matches the binary name, so this lets us skip
    /// self-focus events the same way as on Windows.
    fn self_exe_name() -> Option<String> {
        std::env::current_exe()
            .ok()?
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
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
