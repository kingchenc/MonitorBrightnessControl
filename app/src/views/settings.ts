import { invoke } from "@tauri-apps/api/core";
import { disable, enable, isEnabled } from "@tauri-apps/plugin-autostart";
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

interface Settings {
  initial_brightness: Record<string, number>;
  hotkeys: HotkeySettings;
  auto_dim: AutoDimSettings;
  sync_enabled: boolean;
  sync_primary_id: string | null;
  sync_offsets: Record<string, number>;
  schedules: ScheduleSettings;
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

  const saveBtn = el("button", { type: "button", className: "primary" }, [
    t("settings.save"),
  ]);
  saveBtn.addEventListener("click", async () => {
    try {
      await invoke("save_settings", { settings: s });
      setStatus(t("status.saved.settings"));
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

  let currentlyEnabled = false;
  try {
    currentlyEnabled = await isEnabled();
  } catch (e) {
    console.warn("autostart isEnabled failed", e);
  }

  const row = el("div", { className: "row" });
  row.appendChild(el("label", {}, [t("settings.startup.launch")]));
  const cb = el("input", { type: "checkbox", checked: currentlyEnabled }) as HTMLInputElement;
  cb.addEventListener("change", async () => {
    try {
      if (cb.checked) {
        await enable();
      } else {
        await disable();
      }
      // Reflect the OS truth — some installations refuse the change.
      cb.checked = await isEnabled();
      setStatus(cb.checked ? t("status.saved.settings") : t("status.saved.settings"));
    } catch (e) {
      cb.checked = !cb.checked;
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  row.appendChild(cb);
  card.appendChild(row);
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

function cryptoRandomId(): string {
  const arr = new Uint8Array(8);
  crypto.getRandomValues(arr);
  return Array.from(arr, (b) => b.toString(16).padStart(2, "0")).join("");
}
