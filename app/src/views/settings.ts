import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
import { platform } from "@tauri-apps/plugin-os";
import { el, setStatus } from "../ui";
import { Locale, SUPPORTED_LOCALES, setLocale, t } from "../i18n";

interface AutoDimSettings {
  enabled: boolean;
  latitude: number;
  longitude: number;
  day_brightness: number;
  night_brightness: number;
  transition_minutes: number;
}

interface HotkeySettings {
  brightness_up: string | null;
  brightness_down: string | null;
  toggle_night: string | null;
  toggle_window: string | null;
  blackout: string | null;
  step_percent: number;
  night_brightness_percent: number;
}

interface ScheduleEntry {
  id: string;
  name: string;
  enabled: boolean;
  time: string;
  days: number[];
  monitor_ids: string[];
  brightness_percent: number;
}

interface ScheduleSettings {
  enabled: boolean;
  items: ScheduleEntry[];
}

interface MonitorRow {
  id: string;
  name: string;
  kind: string;
  percent: number | null;
}

interface BackupSettings {
  enabled: boolean;
  retention: number;
}

interface BackupInfo {
  kind: string; // "settings" | "profiles"
  file_name: string;
  created_unix_ms: number;
  size_bytes: number;
}

interface Settings {
  initial_brightness: Record<string, number>;
  hotkeys: HotkeySettings;
  auto_dim: AutoDimSettings;
  sync_enabled: boolean;
  sync_primary_id: string | null;
  sync_offsets: Record<string, number>;
  schedules: ScheduleSettings;
  backup: BackupSettings;
  language: string;
}

/**
 * onLanguageChange is called whenever the user picks a new language so the
 * surrounding chrome (title bar, tabs, footer) can re-render even though
 * we're inside the Settings tab.
 */
export async function renderSettings(
  host: HTMLElement,
  onLanguageChange: () => void = () => {},
) {
  host.replaceChildren();
  let s: Settings;
  try {
    s = await invoke<Settings>("load_settings");
  } catch (e) {
    host.appendChild(
      el("p", { className: "error" }, [`${t("common.error")}: ${e}`]),
    );
    return;
  }
  if (!s.language) s.language = "auto";
  if (!s.schedules) s.schedules = { enabled: false, items: [] };
  if (!Array.isArray(s.schedules.items)) s.schedules.items = [];
  if (!s.backup) s.backup = { enabled: true, retention: 10 };

  let monitors: MonitorRow[] = [];
  try {
    monitors = await invoke<MonitorRow[]>("list_monitors");
  } catch (e) {
    console.warn("list_monitors failed", e);
  }

  host.appendChild(await buildStartupForm(s));
  host.appendChild(buildLanguageForm(s, host, onLanguageChange));
  host.appendChild(buildHotkeyForm(s));
  host.appendChild(buildAutoDimForm(s));
  host.appendChild(buildSchedulesForm(s, monitors));
  host.appendChild(buildSyncForm(s));
  host.appendChild(await buildBackupForm(s, host, onLanguageChange));

  const saveBtn = el("button", { type: "button", className: "primary" }, [
    t("settings.save"),
  ]);
  saveBtn.addEventListener("click", async () => {
    try {
      await invoke("save_settings", { settings: s });
      setStatus(t("status.saved.settings"));
      // Re-render so the backups list reflects the snapshot just taken on save
      // (when backups are enabled), preserving the scroll position.
      const main = document.querySelector("main");
      const scroll = main?.scrollTop ?? 0;
      await renderSettings(host, onLanguageChange);
      if (main) main.scrollTop = scroll;
    } catch (e) {
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  host.appendChild(saveBtn);
}

async function buildStartupForm(_s: Settings): Promise<HTMLElement> {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.startup")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.startup.launch.desc")]));

  let isWindows = false;
  try {
    isWindows = platform() === "windows";
  } catch {
    isWindows = false;
  }

  let normalEnabled = false;
  try {
    normalEnabled = await isEnabled();
  } catch (e) {
    console.warn("autostart isEnabled failed", e);
  }

  let adminEnabled = false;
  if (isWindows) {
    try {
      adminEnabled = await invoke<boolean>("admin_autostart_status");
    } catch (e) {
      console.warn("admin_autostart_status failed", e);
    }
  }

  // --- Normal login-item autostart ---
  const row = el("div", { className: "row" });
  const normalLabel = el("label", {}, [t("settings.startup.launch")]);
  row.appendChild(normalLabel);
  const cb = el("input", { type: "checkbox", checked: normalEnabled }) as HTMLInputElement;
  // Hint shown while the elevated task overrides the login item.
  const overrideHint = el("p", { className: "muted small" }, [
    t("settings.startup.overridden"),
  ]);
  overrideHint.style.display = "none";
  // Will be assigned below once the admin checkbox exists.
  let adminCb: HTMLInputElement | null = null;

  // Reflect the override: while the elevated task is active the login-item
  // option is fully disabled (unchecked + greyed) because it is superseded.
  const applyOverride = (adminOn: boolean) => {
    cb.disabled = adminOn;
    normalLabel.classList.toggle("disabled", adminOn);
    overrideHint.style.display = adminOn ? "" : "none";
    if (adminOn) cb.checked = false;
  };

  cb.addEventListener("change", async () => {
    try {
      if (cb.checked) {
        await enable();
        // Mutually exclusive with the elevated task.
        if (isWindows && adminCb && adminCb.checked) {
          await invoke("set_admin_autostart", { enabled: false });
          adminCb.checked = false;
          applyOverride(false);
        }
      } else {
        await disable();
      }
      cb.checked = await isEnabled();
      setStatus(t("status.saved.settings"));
    } catch (e) {
      cb.checked = !cb.checked;
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  row.appendChild(cb);
  card.appendChild(row);
  card.appendChild(overrideHint);

  // --- Elevated Task Scheduler autostart (Windows only) ---
  if (isWindows) {
    card.appendChild(
      el("p", { className: "muted small" }, [t("settings.startup.admin.desc")]),
    );
    const adminRow = el("div", { className: "row" });
    adminRow.appendChild(el("label", {}, [t("settings.startup.admin")]));
    adminCb = el("input", { type: "checkbox", checked: adminEnabled }) as HTMLInputElement;
    const localAdminCb = adminCb;
    localAdminCb.addEventListener("change", async () => {
      const want = localAdminCb.checked;
      try {
        const now = await invoke<boolean>("set_admin_autostart", { enabled: want });
        localAdminCb.checked = now;
        if (now) {
          // The elevated task fully overrides the login item: remove the
          // non-elevated entry and lock its checkbox so the two can never be
          // active at once (avoids a double launch).
          try {
            if (await isEnabled()) await disable();
          } catch {
            /* ignore */
          }
          applyOverride(true);
          setStatus(t("status.startup.admin_enabled"));
        } else {
          applyOverride(false);
          setStatus(t("status.startup.admin_disabled"));
        }
      } catch (e) {
        // Revert the checkbox to the real state (UAC cancelled, etc.).
        try {
          const real = await invoke<boolean>("admin_autostart_status");
          localAdminCb.checked = real;
          applyOverride(real);
        } catch {
          localAdminCb.checked = !want;
        }
        setStatus(`${t("common.error")}: ${e}`);
      }
    });
    adminRow.appendChild(localAdminCb);
    card.appendChild(adminRow);
  }

  // Apply the initial override state once both checkboxes exist.
  applyOverride(adminEnabled);

  return card;
}

function buildLanguageForm(
  s: Settings,
  host: HTMLElement,
  onLanguageChange: () => void,
): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.language")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.language.note")]));

  const row = el("div", { className: "row" });
  row.appendChild(el("label", {}, [t("settings.language")]));
  const select = el("select", {}, []) as HTMLSelectElement;
  const auto = el("option", { value: "auto" }, [t("settings.language.auto")]);
  select.appendChild(auto);
  for (const loc of SUPPORTED_LOCALES) {
    const opt = el("option", { value: loc.code }, [loc.native]);
    select.appendChild(opt);
  }
  select.value = s.language || "auto";
  select.addEventListener("change", () => {
    s.language = select.value;
    if (s.language !== "auto") {
      setLocale(s.language as Locale);
    }
    // Re-render the settings panel + the chrome so the change is visible
    // without a click on Save.
    onLanguageChange();
    renderSettings(host, onLanguageChange);
  });
  row.appendChild(select);
  card.appendChild(row);
  return card;
}

function buildHotkeyForm(s: Settings): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.hotkeys")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.hotkeys.desc")]));
  card.appendChild(
    inputRow(t("settings.hotkeys.brightness_up"), s.hotkeys.brightness_up ?? "", (v) => {
      s.hotkeys.brightness_up = v || null;
    }),
  );
  card.appendChild(
    inputRow(t("settings.hotkeys.brightness_down"), s.hotkeys.brightness_down ?? "", (v) => {
      s.hotkeys.brightness_down = v || null;
    }),
  );
  card.appendChild(
    inputRow(t("settings.hotkeys.toggle_night"), s.hotkeys.toggle_night ?? "", (v) => {
      s.hotkeys.toggle_night = v || null;
    }),
  );
  card.appendChild(
    inputRow(t("settings.hotkeys.toggle_window"), s.hotkeys.toggle_window ?? "", (v) => {
      s.hotkeys.toggle_window = v || null;
    }),
  );
  card.appendChild(
    inputRow(t("settings.hotkeys.blackout"), s.hotkeys.blackout ?? "", (v) => {
      s.hotkeys.blackout = v || null;
    }),
  );
  card.appendChild(
    numberRow(t("settings.hotkeys.step_percent"), s.hotkeys.step_percent, 0, 50, (v) => {
      s.hotkeys.step_percent = v;
    }),
  );
  card.appendChild(
    numberRow(
      t("settings.hotkeys.night_brightness"),
      s.hotkeys.night_brightness_percent,
      0,
      100,
      (v) => {
        s.hotkeys.night_brightness_percent = v;
      },
    ),
  );
  return card;
}

function buildAutoDimForm(s: Settings): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.autodim")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.autodim.desc")]));
  card.appendChild(
    boolRow(t("settings.autodim.enabled"), s.auto_dim.enabled, (v) => {
      s.auto_dim.enabled = v;
    }),
  );
  card.appendChild(
    numberRow(t("settings.autodim.lat"), s.auto_dim.latitude, -90, 90, (v) => {
      s.auto_dim.latitude = v;
    }),
  );
  card.appendChild(
    numberRow(t("settings.autodim.lon"), s.auto_dim.longitude, -180, 180, (v) => {
      s.auto_dim.longitude = v;
    }),
  );
  card.appendChild(
    numberRow(t("settings.autodim.day"), s.auto_dim.day_brightness, 0, 100, (v) => {
      s.auto_dim.day_brightness = Math.round(v);
    }),
  );
  card.appendChild(
    numberRow(t("settings.autodim.night"), s.auto_dim.night_brightness, 0, 100, (v) => {
      s.auto_dim.night_brightness = Math.round(v);
    }),
  );
  card.appendChild(
    numberRow(t("settings.autodim.transition"), s.auto_dim.transition_minutes, 1, 240, (v) => {
      s.auto_dim.transition_minutes = Math.round(v);
    }),
  );
  return card;
}

function buildSyncForm(s: Settings): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.sync")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.sync.desc")]));
  card.appendChild(
    boolRow(t("settings.sync.enabled"), s.sync_enabled, (v) => {
      s.sync_enabled = v;
    }),
  );
  return card;
}

function inputRow(label: string, value: string, set: (v: string) => void): HTMLElement {
  const row = el("div", { className: "row" });
  row.appendChild(el("label", {}, [label]));
  const i = el("input", { type: "text", value }) as HTMLInputElement;
  i.addEventListener("change", () => set(i.value));
  row.appendChild(i);
  return row;
}

function numberRow(
  label: string,
  value: number,
  min: number,
  max: number,
  set: (v: number) => void,
): HTMLElement {
  const row = el("div", { className: "row" });
  row.appendChild(el("label", {}, [label]));
  const i = el("input", {
    type: "number",
    value: String(value),
    min: String(min),
    max: String(max),
    step: "0.01",
  }) as HTMLInputElement;
  i.addEventListener("change", () => {
    const v = Number(i.value);
    if (!Number.isNaN(v)) set(v);
  });
  row.appendChild(i);
  return row;
}

function boolRow(label: string, value: boolean, set: (v: boolean) => void): HTMLElement {
  const row = el("div", { className: "row" });
  row.appendChild(el("label", {}, [label]));
  const i = el("input", { type: "checkbox", checked: value }) as HTMLInputElement;
  i.addEventListener("change", () => set(i.checked));
  row.appendChild(i);
  return row;
}

function buildSchedulesForm(s: Settings, monitors: MonitorRow[]): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.schedules")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.schedules.desc")]));
  card.appendChild(
    boolRow(t("settings.schedules.enabled"), s.schedules.enabled, (v) => {
      s.schedules.enabled = v;
    }),
  );

  const list = el("div", { className: "schedule-list" });
  card.appendChild(list);

  function repaint() {
    list.replaceChildren();
    for (const entry of s.schedules.items) {
      list.appendChild(scheduleRow(entry, s.schedules, monitors, repaint));
    }
  }
  repaint();

  const addBtn = el("button", { type: "button" }, [t("settings.schedules.add")]);
  addBtn.addEventListener("click", () => {
    s.schedules.items.push({
      id: cryptoRandomId(),
      name: "",
      enabled: true,
      time: "08:00",
      days: [0, 1, 2, 3, 4, 5, 6],
      monitor_ids: [],
      brightness_percent: 80,
    });
    repaint();
  });
  card.appendChild(addBtn);

  return card;
}

function scheduleRow(
  entry: ScheduleEntry,
  schedules: ScheduleSettings,
  monitors: MonitorRow[],
  repaint: () => void,
): HTMLElement {
  const wrap = el("div", { className: "schedule-item card" });

  const head = el("div", { className: "row" });
  const enabled = el("input", {
    type: "checkbox",
    checked: entry.enabled,
  }) as HTMLInputElement;
  enabled.addEventListener("change", () => {
    entry.enabled = enabled.checked;
  });
  head.appendChild(enabled);

  const nameIn = el("input", {
    type: "text",
    value: entry.name,
    placeholder: t("settings.schedules.name_placeholder"),
  }) as HTMLInputElement;
  nameIn.addEventListener("change", () => {
    entry.name = nameIn.value;
  });
  head.appendChild(nameIn);

  const time = el("input", { type: "time", value: entry.time }) as HTMLInputElement;
  time.addEventListener("change", () => {
    if (time.value) entry.time = time.value;
  });
  head.appendChild(time);

  const del = el("button", { type: "button", className: "danger" }, [
    t("profiles.delete"),
  ]);
  del.addEventListener("click", () => {
    schedules.items = schedules.items.filter((it) => it.id !== entry.id);
    repaint();
  });
  head.appendChild(del);
  wrap.appendChild(head);

  // Days of the week (Sun=0 .. Sat=6).
  const dayKeys = [
    "settings.schedules.day.sun",
    "settings.schedules.day.mon",
    "settings.schedules.day.tue",
    "settings.schedules.day.wed",
    "settings.schedules.day.thu",
    "settings.schedules.day.fri",
    "settings.schedules.day.sat",
  ];
  const daysRow = el("div", { className: "row schedule-days" });
  daysRow.appendChild(el("label", {}, [t("settings.schedules.days")]));
  const daysChips = el("div", { className: "chip-row" });
  for (let i = 0; i < 7; i++) {
    const lbl = el("label", { className: "chip" });
    const cb = el("input", {
      type: "checkbox",
      checked: entry.days.includes(i),
    }) as HTMLInputElement;
    cb.addEventListener("change", () => {
      if (cb.checked) {
        if (!entry.days.includes(i)) entry.days.push(i);
        entry.days.sort();
      } else {
        entry.days = entry.days.filter((d) => d !== i);
      }
    });
    lbl.appendChild(cb);
    lbl.appendChild(document.createTextNode(t(dayKeys[i])));
    daysChips.appendChild(lbl);
  }
  daysRow.appendChild(daysChips);
  wrap.appendChild(daysRow);

  // Brightness slider.
  const brightRow = el("div", { className: "row" });
  brightRow.appendChild(el("label", {}, [t("settings.schedules.brightness")]));
  const range = el("input", {
    type: "range",
    min: "0",
    max: "100",
    step: "1",
    value: String(entry.brightness_percent),
  }) as HTMLInputElement;
  const readout = el("span", { className: "muted" }, [`${entry.brightness_percent}%`]);
  range.addEventListener("input", () => {
    const v = Math.round(Number(range.value));
    entry.brightness_percent = v;
    readout.textContent = `${v}%`;
  });
  brightRow.appendChild(range);
  brightRow.appendChild(readout);
  wrap.appendChild(brightRow);

  // Monitor selection — empty list = all.
  const monRow = el("div", { className: "row schedule-monitors" });
  monRow.appendChild(el("label", {}, [t("settings.schedules.monitors")]));
  const monChips = el("div", { className: "chip-row" });

  const allLbl = el("label", { className: "chip" });
  const allCb = el("input", {
    type: "checkbox",
    checked: entry.monitor_ids.length === 0,
  }) as HTMLInputElement;
  allCb.addEventListener("change", () => {
    if (allCb.checked) {
      entry.monitor_ids = [];
      for (const cb of perMonitorBoxes) cb.checked = false;
    } else if (entry.monitor_ids.length === 0) {
      // Re-check it — "all" can't be unchecked unless individual ones are picked.
      allCb.checked = true;
    }
  });
  allLbl.appendChild(allCb);
  allLbl.appendChild(document.createTextNode(t("settings.schedules.monitors.all")));
  monChips.appendChild(allLbl);

  const perMonitorBoxes: HTMLInputElement[] = [];
  if (monitors.length === 0) {
    monChips.appendChild(
      el("span", { className: "muted small" }, [t("settings.schedules.monitors.none")]),
    );
  } else {
    for (const m of monitors) {
      const lbl = el("label", { className: "chip" });
      const cb = el("input", {
        type: "checkbox",
        checked: entry.monitor_ids.includes(m.id),
      }) as HTMLInputElement;
      cb.addEventListener("change", () => {
        if (cb.checked) {
          if (!entry.monitor_ids.includes(m.id)) entry.monitor_ids.push(m.id);
          allCb.checked = false;
        } else {
          entry.monitor_ids = entry.monitor_ids.filter((id) => id !== m.id);
          if (entry.monitor_ids.length === 0) allCb.checked = true;
        }
      });
      perMonitorBoxes.push(cb);
      lbl.appendChild(cb);
      lbl.appendChild(document.createTextNode(m.name || m.id));
      monChips.appendChild(lbl);
    }
  }
  monRow.appendChild(monChips);
  wrap.appendChild(monRow);

  return wrap;
}

async function buildBackupForm(
  s: Settings,
  host: HTMLElement,
  onLanguageChange: () => void,
): Promise<HTMLElement> {
  const card = el("article", { className: "card" });
  card.appendChild(el("h2", {}, [t("settings.backup")]));
  card.appendChild(el("p", { className: "muted" }, [t("settings.backup.desc")]));

  card.appendChild(
    boolRow(t("settings.backup.enabled"), s.backup.enabled, (v) => {
      s.backup.enabled = v;
    }),
  );
  card.appendChild(
    numberRow(t("settings.backup.retention"), s.backup.retention, 1, 100, (v) => {
      s.backup.retention = Math.max(1, Math.round(v));
    }),
  );

  const actions = el("div", { className: "row" });
  const nowBtn = el("button", { type: "button" }, [t("settings.backup.now")]);
  nowBtn.addEventListener("click", async () => {
    try {
      await invoke<BackupInfo[]>("backup_settings_now");
      setStatus(t("status.backup.created"));
      renderSettings(host, onLanguageChange);
    } catch (e) {
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  actions.appendChild(nowBtn);
  card.appendChild(actions);

  // Existing backups list.
  let backups: BackupInfo[] = [];
  try {
    backups = await invoke<BackupInfo[]>("list_settings_backups");
  } catch (e) {
    console.warn("list_settings_backups failed", e);
  }

  const heading = el("div", { className: "row" });
  heading.appendChild(el("label", {}, [t("settings.backup.list")]));
  heading.appendChild(
    el("span", { className: "muted small" }, [
      `${backups.length} ${t("settings.backup.count")}`,
    ]),
  );
  card.appendChild(heading);

  if (backups.length === 0) {
    card.appendChild(el("p", { className: "muted small" }, [t("settings.backup.none")]));
    return card;
  }

  const list = el("div", { className: "backup-list" });
  for (const b of backups) {
    list.appendChild(backupRow(b, host, onLanguageChange));
  }
  card.appendChild(list);
  return card;
}

function backupRow(
  b: BackupInfo,
  host: HTMLElement,
  onLanguageChange: () => void,
): HTMLElement {
  const row = el("div", { className: "row backup-item" });
  const isProfiles = b.kind === "profiles";
  const when = b.created_unix_ms > 0 ? new Date(b.created_unix_ms).toLocaleString() : b.file_name;
  const sizeKb = (b.size_bytes / 1024).toFixed(1);
  row.appendChild(
    el("span", { className: "kind-badge" }, [
      isProfiles ? t("tab.profiles") : t("tab.settings"),
    ]),
  );
  row.appendChild(el("span", {}, [`${when}`]));
  row.appendChild(el("span", { className: "muted small" }, [`${sizeKb} KB`]));

  const btns = el("div", { className: "backup-actions" });
  const restore = el("button", { type: "button" }, [t("settings.backup.restore")]);
  restore.addEventListener("click", async () => {
    try {
      // Restore the matching config file. Profiles and settings have separate
      // commands so each refreshes the right in-memory state.
      const cmd = isProfiles ? "restore_profiles_backup" : "restore_settings_backup";
      await invoke(cmd, { fileName: b.file_name });
      setStatus(t("status.backup.restored"));
      // Reload the whole settings view and chrome from the restored state.
      onLanguageChange();
      renderSettings(host, onLanguageChange);
    } catch (e) {
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  const del = el("button", { type: "button", className: "danger" }, [
    t("settings.backup.delete"),
  ]);
  del.addEventListener("click", async () => {
    try {
      await invoke("delete_settings_backup", { fileName: b.file_name });
      setStatus(t("status.backup.deleted"));
      renderSettings(host, onLanguageChange);
    } catch (e) {
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  btns.appendChild(restore);
  btns.appendChild(del);
  row.appendChild(btns);
  return row;
}

function cryptoRandomId(): string {
  const arr = new Uint8Array(8);
  crypto.getRandomValues(arr);
  return Array.from(arr, (b) => b.toString(16).padStart(2, "0")).join("");
}
