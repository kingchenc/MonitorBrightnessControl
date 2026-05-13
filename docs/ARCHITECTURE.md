# Architecture

```
┌────────────────────────────────────────────────────────────────┐
│ frontend (TypeScript + Vite)                                   │
│   monitors / settings / profiles / about views                 │
└──────────────┬─────────────────────────────────────────────────┘
               │ Tauri IPC (invoke)
┌──────────────▼─────────────────────────────────────────────────┐
│ app/src-tauri (monitor_brightness_control_lib)                 │
│   commands.rs          Tauri command handlers + event emit     │
│   state.rs             AppState — manager + brightness cache   │
│   tray.rs              tray icon + menu (per-monitor + profile)│
│   hotkeys.rs           global-shortcut plugin wiring           │
│   profiles.rs          per-app monitor overrides (brightness / │
│                        contrast / color preset)                │
│   foreground.rs        foreground-window watcher (per OS)      │
│   auto_dim.rs          sun-position based brightness curve     │
│   scheduler.rs         time-of-day brightness rules            │
│   config.rs            TOML persistence (settings + profiles)  │
└──────────────┬─────────────────────────────────────────────────┘
               │ direct Rust calls
┌──────────────▼─────────────────────────────────────────────────┐
│ brightness-core                                                │
│   monitor::MonitorManager / MonitorHandle traits               │
│   ddc.rs               DDC/CI wire encoding (XOR checksum)     │
│   caps.rs              MCCS capability-string parser           │
│   vcp.rs               VCP feature codes + percent math        │
│   platform/                                                    │
│     windows.rs         dxva2 + WMI worker thread               │
│     macos.rs           DisplayServices + IOAVService           │
│     linux.rs           sysfs backlight + /dev/i2c              │
│     stub.rs            unsupported-platform fallback           │
└────────────────────────────────────────────────────────────────┘
```

## Why not just `SetMonitorBrightness`?

The Win32 documented function `SetMonitorBrightness(HANDLE, DWORD)` is the simplest path, but it has problems for a daily-driver app:

1. It only writes VCP code 0x10. We also want to read it (`GetMonitorBrightness`) and to drive contrast (0x12), color preset (0x14), input source (0x60), volume (0x62) etc. — the dedicated APIs do not exist for those.
2. Internally `SetMonitorBrightness` calls `SetVCPFeature(... 0x10 ...)`. There is no perf advantage over the lower-level API.
3. The slow part of every call is `EnumDisplayMonitors → GetPhysicalMonitorsFromHMONITOR`. We pay that **once** at startup (and on `WM_DEVICECHANGE`) and reuse the handles forever.

Result: a brightness change is one `SetVCPFeature` call (~10–30 ms instead of 200–500 ms for tools that re-enumerate every time).

## Why a worker thread for WMI?

`IWbemServices` (and therefore the `wmi` crate's `WMIConnection`) is COM-apartment-bound. It is `!Send`. Building an `Arc<dyn MonitorHandle: Send + Sync>` wrapper around it requires either:

* opening a fresh connection on every call (≈ 100–200 ms each), or
* dedicating a thread that owns the connection forever and routes requests over a channel.

We do the second. One OS thread per laptop panel (i.e. one) keeps every backlight read/write under 5 ms and avoids any Send/Sync hazard.

## Why `IOAVService` on macOS?

The traditional `IOFramebuffer`-based DDC/CI path stopped working reliably on Apple Silicon because the display controller (`AppleCLCD2`) no longer exposes user-space-friendly I²C operations. `IOAVService` is the path the OS itself uses for HDMI/DP-on-USB-C output, and its `IOAVServiceWriteI2C` / `ReadI2C` symbols are stable enough that the open-source community standardized on them (m1ddc, MonitorControl).

We pair `IOAVServiceCreateWithService` with the `DCPAVServiceProxy` registry entries whose `Location == "External"` to skip the internal-display proxy that also exists on AS.

## DDC/CI checksum primer

The protocol travels over I²C address `0x37` (display destination) / `0x6E` (host source written by the display). Each frame ends with a single XOR checksum byte:

```
host  → display:  seed = 0x6E (= 0x37 << 1)
display → host:   seed = 0x6E (= 0x37 << 1; same byte coincidentally)
```

`brightness-core::ddc` encodes the seed and verifies the checksum on every reply. The unit tests pin a known-good frame from the spec so a regression in the encoder is caught immediately.

## Why startup defers monitor enumeration

`AppState::initialize()` is called before `tauri::Builder` runs, so any work it does blocks the splash screen. Enumerating monitors involves:

* a synchronous WMI probe for the internal panel (COM init + `SELECT … FROM WmiMonitorBrightness`, 500–2000 ms on first launch, longer when WMI returns `HRESULT 0x8004100C`);
* `GetMonitorCapabilities` per external monitor (200–500 ms each, DDC/CI roundtrip).

We therefore initialize `AppState` with an empty monitor list and spawn a `startup-brightness` worker thread from the Tauri `setup` hook. The worker calls `refresh_monitors → refresh_brightness_cache → apply_initial_settings`, emitting a `monitors-changed` event after each stage and rebuilding the tray menu. The frontend listens for the event and re-renders the active tab once data arrives. Tray and main window therefore appear instantly, even on machines where WMI is slow.

## The brightness cache

`AppState.brightness_cache: RwLock<HashMap<String, f32>>` stores the last-known brightness percentage per monitor id. Every read path (`rows()`, the tray menu, the frontend's monitor list) consumes the cache rather than triggering a fresh DDC/CI roundtrip. Writes (`set_brightness`, `step_brightness`) update the cache after a successful set so subsequent reads stay coherent. A manual "Refresh" or a `WM_DEVICECHANGE` rebuilds the cache from hardware via `refresh_brightness_cache()`. This cuts the per-tab-render cost from "seconds of DDC reads" to "memory copy".

## Why quitting needs an explicit flag

`RunEvent::ExitRequested` fires for every exit attempt, including when the user closes the main window. We want the window-close case to keep the app alive in the tray, but a tray-Quit click to actually exit. The solution is `AppState.quitting: AtomicBool`. The tray "Quit" item and the `quit_app` command set the flag before calling `app.exit(0)`; the run-loop handler vetoes the exit (`api.prevent_exit()`) only when the flag is unset. Without this, the previous version's blanket `prevent_exit()` made Quit silently ignored.

## Why per-app profiles use polling on Windows

`SetWinEventHook(EVENT_SYSTEM_FOREGROUND)` runs in a system message-pump context. To pump events you need either a window or a dedicated message-only thread, plus careful COM apartment handling. For a feature where 750 ms latency is invisible to the user, a polling thread on `GetForegroundWindow()` is far simpler with no functional downside.

On Linux the same polling strategy reads `_NET_ACTIVE_WINDOW` via `xprop`. Wayland has no portable equivalent (each compositor exposes its own protocol), so the watcher is silently inactive on Wayland sessions — a future Mutter/Plasma extension could close that gap.
