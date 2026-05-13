# Screenshots

Drop screenshots here with the exact filenames the README references:

| File | What to capture | Suggested size |
|---|---|---|
| `tray.png` | System-tray icon expanded — per-monitor brightness submenus, "Apply profile" submenu (with at least one profile), "Toggle night mode", "Show window", "Quit". | ~280 × 500 px |
| `monitors.png` | Monitors tab with at least two monitors: brightness sliders, contrast slider and color-preset dropdown for external displays. | ~720 × 540 px |
| `settings.png` | Settings tab showing Startup, Language, Hotkeys, Auto-dim, Schedules (with one or two entries expanded) and Multi-monitor sync cards. | ~720 × 900 px |
| `profiles.png` | Profiles tab with one profile expanded — name, optional app identifier, per-monitor override cards with the brightness/contrast/color sliders. | ~720 × 720 px |
| `about.png` | About tab. | ~720 × 360 px |

Format: PNG, 1× DPI is fine. Light or dark mode — pick whichever looks cleaner; the README captions are theme-agnostic.

After saving them, run `npm run build && cargo build --release -p monitor-brightness-control --features custom-protocol` is **not** required — the README only references these files, the app itself doesn't bundle them.
