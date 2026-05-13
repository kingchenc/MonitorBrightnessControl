# Privacy Policy

Monitor Brightness Control runs entirely on your device. It does not transmit personal information, telemetry, analytics, crash reports or any other data to the developer or to third parties.

## What the app stores locally

* `<config>/MonitorBrightnessControl/settings.toml` — UI language, hotkeys, auto-dim coordinates and brightness levels, time-of-day schedules (HH:MM, weekdays, target monitors, brightness percentage), multi-monitor sync settings, and per-monitor startup brightness defaults.
* `<config>/MonitorBrightnessControl/profiles.toml` — your per-application monitor profiles: profile name, optional application identifier, and per-monitor overrides for brightness / contrast / color preset.

`<config>` is the OS-default configuration directory:

* Windows: `%APPDATA%\MonitorBrightnessControl\`
* macOS: `~/Library/Application Support/MonitorBrightnessControl/`
* Linux: `$XDG_CONFIG_HOME/MonitorBrightnessControl/` (default `~/.config/MonitorBrightnessControl/`)

Both files are plain TOML; you can read, edit or delete them at any time. The app also keeps an **in-memory** brightness cache (last-known percentage per monitor) for the duration of a session; this is never written to disk.

## What the app reads from your computer

* The list of connected monitors and their EDID / MCCS capabilities, in order to drive their brightness, contrast and color preset.
* The basename of the executable (Windows) / bundle identifier (macOS) / `WM_CLASS` (Linux X11) of the currently focused window — only when **per-app profiles** have a non-empty application identifier — to look up which profile to apply. The value is **not** stored or transmitted; it is matched in memory and discarded.
* The current local time, used by the time-of-day scheduler to decide whether a schedule entry should fire.

## What the app sends over the network

Nothing. There is no auto-update channel, telemetry endpoint, or crash reporter built into the application.

## Contact

For privacy questions, open an issue at the project's GitHub repository.
