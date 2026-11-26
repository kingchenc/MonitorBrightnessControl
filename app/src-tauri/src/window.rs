//! Main window lifecycle: built on demand, destroyed on close.
//!
//! The app lives in the tray and the window is closed most of the time. A
//! hidden-but-alive window keeps a whole WebView2/WebKit process tree around
//! (on Windows six `msedgewebview2` processes, ~180 MB), so the window is not
//! created at startup (`"create": false` in `tauri.conf.json`), is built from
//! that config when the user opens it, and is destroyed when it is closed.
//! Everything the page shows lives in the backend (`AppState`), so a freshly
//! built page renders the same state the old one had.

use std::sync::Mutex;

use tauri::{AppHandle, Manager, WebviewWindow, WebviewWindowBuilder};

pub const MAIN: &str = "main";

/// Serializes window creation so two quick clicks never build two windows.
static BUILD: Mutex<()> = Mutex::new(());

/// Show the main window, building it first if it does not exist.
///
/// Safe to call from the event loop (tray, menu, single-instance and hotkey
/// handlers): building a webview there deadlocks on Windows, so a missing
/// window is built on a short-lived thread.
pub fn show(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(MAIN) {
        focus(&w);
        return;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("open-window".into())
        .spawn(move || {
            let _guard = BUILD.lock().unwrap_or_else(|e| e.into_inner());
            let window = match app.get_webview_window(MAIN) {
                Some(w) => w,
                None => match build(&app) {
                    Ok(w) => w,
                    Err(e) => {
                        log::error!("build main window: {e}");
                        return;
                    }
                },
            };
            focus(&window);
        });
    if let Err(e) = spawned {
        log::error!("spawn open-window thread: {e}");
    }
}

/// Close the main window if it is open (it is destroyed, see the module
/// docs), otherwise open it.
pub fn toggle(app: &AppHandle) {
    match app.get_webview_window(MAIN) {
        Some(w) if w.is_visible().unwrap_or(false) => {
            let _ = w.close();
        }
        _ => show(app),
    }
}

fn build(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == MAIN)
        .cloned()
        .ok_or_else(|| tauri::Error::WindowNotFound)?;
    WebviewWindowBuilder::from_config(app, &config)?.build()
}

fn focus(w: &WebviewWindow) {
    // Order matters: unminimize *before* set_focus, and focus last, so the
    // freshly shown window actually owns keyboard/mouse focus. Otherwise the
    // first click in the window only activates it and is swallowed instead
    // of hitting the control under the cursor.
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
}
