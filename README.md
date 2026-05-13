# Monitor Brightness Control

A small, fast desktop app for controlling brightness, contrast, color temperature and input source on every monitor connected to your machine — your laptop's built-in panel **and** every external DDC/CI display. Written in Rust, packaged with Tauri 2; the shipped binary is ~7 MB and has no Electron or Node runtime.

## Features

- **All your monitors, one window.** Per-display sliders for brightness; for external monitors, also contrast and the standard color presets (sRGB, Native, 5000K – 9300K, User).
- **System tray.** Right-click for per-monitor brightness submenus, quick "Apply profile" picker (visible as soon as you have one), night-mode toggle, and a one-click way to bring the window back.
- **Global hotkeys.** Brightness ±, night mode, blackout, show/hide window — all configurable; defaults to `Ctrl+Shift+Up`/`Down`/`N`/`B`.
- **Per-app profiles.** Bind a profile to an executable (`firefox.exe`, `cs2.exe`, `com.apple.Safari`, the WM_CLASS on Linux) and it auto-applies whenever that app gets focus. Or leave the app id empty and trigger the profile manually from the tray menu.
- **Schedules.** Time-of-day rules — apply a brightness preset at 08:00 on weekdays, dim everything after 22:00, target individual monitors or all of them. Times use your system's local timezone.
- **Auto-dim.** Smoothly transitions between day and night brightness around your local sunrise/sunset for the configured latitude/longitude. Fully offline, sun-position math runs locally.
- **Multi-monitor sync.** Optionally keep secondary displays at a fixed offset from your primary.
- **Six languages out of the box.** English, German, Spanish, French, Italian, Japanese. The app reads your OS locale by default.
- **Autostart toggle.** First-class operating-system autostart (no scheduled-tasks-XML hackery).
- **CLI (`mbc`).** Drives the same Rust core, scriptable from your dotfiles, Home Assistant, AutoHotkey, etc.

## Screenshots

> _Placeholder images — drop the real PNGs under [`docs/screenshots/`](docs/screenshots/) (see [its README](docs/screenshots/README.md) for sizes and what each shot should contain)._

| | |
|---|---|
| ![System tray expanded](docs/screenshots/tray.png) | ![Monitors tab](docs/screenshots/monitors.png) |
| **System tray** — per-monitor submenus + Apply-profile picker. | **Monitors** — brightness, contrast and color-preset controls per display. |
| ![Settings tab](docs/screenshots/settings.png) | ![Profiles tab](docs/screenshots/profiles.png) |
| **Settings** — startup, language, hotkeys, auto-dim, schedules, sync. | **Profiles** — per-app overrides; loads current monitor values when you create a new one. |
| ![About tab](docs/screenshots/about.png) | |
| **About** — version, links, license. | |

## Platform support

| Platform | Internal panel | External (DDC/CI) | Hot-plug | Status |
|-|-|-|-|-|
| Windows 10 / 11 | WMI `WmiMonitorBrightnessMethods` | `dxva2` Low-Level Monitor Configuration API with cached `PHYSICAL_MONITOR` handles | `WM_DEVICECHANGE` (cache invalidation) | ✅ Primary development target — Windows 11 Pro 26200 with 3× DDC/CI displays |
| macOS 11+ | `DisplayServices.framework` (private), `CoreDisplay_Display_SetUserBrightness` fallback | `IOAVService` over `DCPAVServiceProxy` (m1ddc-style path) | `IOServiceMatching` re-enumeration | ⚠ Compiles; needs hardware to validate the IOAVService pairing |
| Linux | `/sys/class/backlight/*/brightness` | `/dev/i2c-*` with `I2C_SLAVE` ioctl | `udev` (`drm`, `i2c-dev`) | ⚠ Compiles; install `90-monitor-brightness.rules` for non-root use |

## Install

* **Microsoft Store** — _coming soon._
* **Direct download** — pre-built installers on the [Releases page](https://github.com/kingchenc/MonitorBrightnessControl/releases).
* **Linux distros** — AppImage, `.deb`, Flatpak (manifest in [`packaging/linux/`](packaging/linux/)).
* **macOS** — `.dmg`, notarized in CI. Mac App Store submission is **not** recommended because the IOAVService path uses private symbols.

## Build from source

```bash
git clone https://github.com/kingchenc/MonitorBrightnessControl
cd MonitorBrightnessControl

# CLI only — no Node toolchain needed
cargo build --release -p brightness-cli
./target/release/mbc list
./target/release/mbc set --id 'win:\\.\DISPLAY1#0' 60

# Full Tauri app (release exe in target/release/)
cd app && npm install && npm run build && cd ..
cargo build --release -p monitor-brightness-control --features custom-protocol
```

The `--features custom-protocol` flag is what makes a direct `cargo build` produce a working app — `cargo tauri build` would set it automatically. Detailed packaging, signing, MSIX and Flatpak instructions: [`docs/BUILDING.md`](docs/BUILDING.md).

## CLI quick reference

```bash
mbc list                                  # show every monitor + current brightness
mbc set --id <id> 60                      # set one monitor to 60%
mbc set 80                                # set every monitor to 80%
mbc up 10                                 # step every monitor up by 10%
mbc vcp get --id <id> --code 0x12         # read raw VCP feature (contrast in this case)
mbc vcp set --id <id> --code 0x14 --val 5 # set color preset to 6500K
```

## Architecture

`brightness-core` is the entire portable surface: a `MonitorManager` trait with platform-specific backends under [`crates/brightness-core/src/platform/`](crates/brightness-core/src/platform/). Both `brightness-cli` and the Tauri app are thin wrappers around it.

Notable design choices, all documented in [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md):

- DDC/CI handles are cached and serialized with a mutex — the OS APIs are not thread-safe in the way you'd hope.
- Brightness/contrast reads are kept in a per-monitor cache so the UI and tray menu don't pay a 50–300 ms DDC/CI roundtrip on every render.
- Heavy enumeration runs on a background thread after the window appears — startup feels instant even on machines where WMI brightness queries are slow.
- The foreground-window watcher polls (~750 ms on Windows, 1 s on macOS/Linux) rather than hooking — much simpler and the latency is invisible at human time scales.

## Privacy

The app makes no network calls. It does not phone home. It does not collect telemetry. It writes its config to your OS's standard config directory (`%APPDATA%\MonitorBrightnessControl\` on Windows, `~/Library/Application Support/MonitorBrightnessControl/` on macOS, `~/.config/MonitorBrightnessControl/` on Linux). See [`docs/PRIVACY.md`](docs/PRIVACY.md).

## Contributing

Bug reports, platform fixes (especially the macOS IOAVService path) and additional languages are welcome via pull request. See [`CONTRIBUTING.md`](CONTRIBUTING.md) for the full guide, the inbound-license terms, and the development setup.

## Disclaimer

This software is provided **as-is, with no warranty of any kind**. The author does not assume liability for any damage to monitors, hardware, OS settings, or data resulting from use of this software. DDC/CI writes go straight to display firmware; some monitors don't follow the spec cleanly and may behave unexpectedly. **Use at your own risk.**

## License

**Source-available, proprietary.** Copyright © 2026 kingchenc. All rights reserved. See [`LICENSE`](LICENSE).

Quick summary — read the LICENSE for the actual terms:

| You may | You may not (without written permission) |
|---|---|
| Run the official binaries — personal use, internal business use, on any number of devices you own or control. | Modify, adapt or fork the code. |
| Read the source code in this repository. | Redistribute the Software or any modified copy in source or binary form. |
| Submit pull requests, bug reports and feature ideas. | Sell, rent, sublicense or repackage it as a paid product. |
| | Reverse-engineer the binaries (beyond what local law mandates). |

This is **not** OSI-approved Open Source — it is "source-available". The dependencies (Tauri, the Rust ecosystem) remain under their own MIT / Apache-2.0 licenses; their notices are reproduced in [`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md). Only the first-party code in this repository is covered by the proprietary terms above.
