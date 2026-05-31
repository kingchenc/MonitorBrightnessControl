import { invoke } from "@tauri-apps/api/core";
import { el, setStatus } from "../ui";
import { t } from "../i18n";

interface ProfileMonitorSettings {
  brightness: number | null;
  contrast: number | null;
  color_preset: number | null;
}

interface Profile {
  id: string;
  name: string;
  app_id: string;
  monitors: Record<string, ProfileMonitorSettings>;
  brightness: Record<string, number>;
}

interface Profiles {
  items: Profile[];
}

interface MonitorRow {
  id: string;
  name: string;
  kind: string;
  percent: number | null;
}

interface VcpView {
  current: number;
  maximum: number;
}

interface ProfileTemplate {
  id: string;
  name: string;
  brightness: number;
  contrast: number;
  color_preset: number;
}

const VCP_CONTRAST = 0x12;
const VCP_COLOR_PRESET = 0x14;

function colorPresetOptions() {
  return [
    { label: t("monitors.preset.srgb"), value: 1 },
    { label: t("monitors.preset.native"), value: 2 },
    { label: t("monitors.preset.5000k"), value: 4 },
    { label: t("monitors.preset.6500k"), value: 5 },
    { label: t("monitors.preset.7500k"), value: 6 },
    { label: t("monitors.preset.9300k"), value: 8 },
    { label: t("monitors.preset.user"), value: 11 },
  ];
}

export async function renderProfiles(host: HTMLElement) {
  host.replaceChildren();
  let p: Profiles;
  let monitors: MonitorRow[] = [];
  let templates: ProfileTemplate[] = [];
  try {
    p = await invoke<Profiles>("load_profiles");
    monitors = await invoke<MonitorRow[]>("list_monitors");
  } catch (e) {
    host.appendChild(el("p", { className: "error" }, [`${t("common.error")}: ${e}`]));
    return;
  }
  try {
    templates = await invoke<ProfileTemplate[]>("default_profile_templates");
  } catch (e) {
    console.warn("default_profile_templates failed", e);
  }
  // Migrate legacy `brightness` map into the new `monitors` block so the UI
  // can present everything uniformly.
  for (const profile of p.items) {
    if (!profile.monitors) profile.monitors = {};
    if (profile.brightness) {
      for (const [id, pct] of Object.entries(profile.brightness)) {
        if (!profile.monitors[id]) {
          profile.monitors[id] = { brightness: pct, contrast: null, color_preset: null };
        } else if (profile.monitors[id].brightness == null) {
          profile.monitors[id].brightness = pct;
        }
      }
      profile.brightness = {};
    }
  }
  paint(host, p, monitors, templates);
}

function templateLabel(tpl: ProfileTemplate): string {
  const key = `profiles.template.${tpl.id}`;
  const localized = t(key);
  // t() returns the key itself when missing — fall back to the backend name.
  return localized === key ? tpl.name : localized;
}

function profileFromTemplate(tpl: ProfileTemplate, monitors: MonitorRow[]): Profile {
  const fresh: Profile = {
    id: cryptoRandomId(),
    name: templateLabel(tpl),
    app_id: "",
    monitors: {},
    brightness: {},
  };
  for (const m of monitors) {
    fresh.monitors[m.id] = {
      brightness: tpl.brightness,
      // Contrast and color preset only apply to external DDC/CI monitors.
      contrast: m.kind === "external" ? tpl.contrast : null,
      color_preset: m.kind === "external" ? tpl.color_preset : null,
    };
  }
  return fresh;
}

function paint(
  host: HTMLElement,
  p: Profiles,
  monitors: MonitorRow[],
  templates: ProfileTemplate[],
) {
  host.replaceChildren();
  host.appendChild(el("p", { className: "muted" }, [t("profiles.intro")]));

  const list = el("div", { className: "profile-list" });
  for (const profile of p.items) {
    list.appendChild(profileCard(profile, p, monitors, host, templates));
  }
  host.appendChild(list);

  const addBtn = el("button", { type: "button" }, [t("profiles.add")]);
  addBtn.addEventListener("click", async () => {
    const fresh: Profile = {
      id: cryptoRandomId(),
      name: t("profiles.new_default_name"),
      app_id: "",
      monitors: {},
      brightness: {},
    };
    // Pre-populate with each connected monitor's current values so the user
    // can fine-tune from a sensible baseline.
    for (const m of monitors) {
      fresh.monitors[m.id] = await snapshotMonitor(m);
    }
    p.items.push(fresh);
    paint(host, p, monitors, templates);
  });

  // "Add from template" picker.
  if (templates.length > 0) {
    const bar = el("div", { className: "template-bar" });
    const select = el("select", {}) as HTMLSelectElement;
    for (const tpl of templates) {
      select.appendChild(el("option", { value: tpl.id }, [templateLabel(tpl)]));
    }
    const tplBtn = el("button", { type: "button" }, [t("profiles.templates")]);
    tplBtn.addEventListener("click", () => {
      const tpl = templates.find((x) => x.id === select.value);
      if (!tpl) return;
      p.items.push(profileFromTemplate(tpl, monitors));
      setStatus(t("status.profile.template_added"));
      paint(host, p, monitors, templates);
    });
    bar.appendChild(select);
    bar.appendChild(tplBtn);
    bar.appendChild(addBtn);
    host.appendChild(bar);
  } else {
    host.appendChild(addBtn);
  }

  const saveBtn = el("button", { type: "button", className: "primary" }, [t("profiles.save")]);
  saveBtn.addEventListener("click", async () => {
    try {
      await invoke("save_profiles", { profiles: p });
      setStatus(t("status.saved.profiles"));
    } catch (e) {
      setStatus(`${t("common.error")}: ${e}`);
    }
  });
  host.appendChild(saveBtn);
}

async function snapshotMonitor(m: MonitorRow): Promise<ProfileMonitorSettings> {
  const out: ProfileMonitorSettings = {
    brightness: m.percent === null ? 80 : Math.round(m.percent),
    contrast: null,
    color_preset: null,
  };
  if (m.kind === "external") {
    try {
      const v = await invoke<VcpView>("get_vcp", { id: m.id, code: VCP_CONTRAST });
      if (v.maximum > 0) {
        out.contrast = Math.round((v.current / v.maximum) * 100);
      }
    } catch {
      // contrast unsupported on this monitor — leave null
    }
    try {
      const v = await invoke<VcpView>("get_vcp", { id: m.id, code: VCP_COLOR_PRESET });
      out.color_preset = v.current;
    } catch {
      // color preset unsupported — leave null
    }
  }
  return out;
}

function profileCard(
  profile: Profile,
  all: Profiles,
  monitors: MonitorRow[],
  host: HTMLElement,
  templates: ProfileTemplate[],
): HTMLElement {
  const card = el("article", { className: "card" });
  card.appendChild(el("h3", {}, [profile.name || t("profiles.unnamed")]));

  const nameRow = el("div", { className: "row" });
  nameRow.appendChild(el("label", {}, [t("profiles.name")]));
  const nameInput = el("input", { type: "text", value: profile.name }) as HTMLInputElement;
  nameInput.addEventListener("change", () => {
    profile.name = nameInput.value;
  });
  nameRow.appendChild(nameInput);
  card.appendChild(nameRow);

  const idRow = el("div", { className: "row" });
  idRow.appendChild(el("label", {}, [t("profiles.app_id")]));
  const appWrap = el("div");
  const appInput = el("input", {
    type: "text",
    value: profile.app_id,
    placeholder: t("profiles.app_id_placeholder"),
  }) as HTMLInputElement;
  appInput.addEventListener("change", () => {
    profile.app_id = appInput.value;
  });
  appWrap.appendChild(appInput);
  appWrap.appendChild(
    el("p", { className: "muted small" }, [t("profiles.app_id_hint")]),
  );
  idRow.appendChild(appWrap);
  card.appendChild(idRow);

  if (monitors.length === 0) {
    card.appendChild(
      el("p", { className: "muted small" }, [t("profiles.no_monitors")]),
    );
  } else {
    for (const m of monitors) {
      card.appendChild(monitorOverride(profile, m));
    }
  }

  const reloadBtn = el("button", { type: "button" }, [t("profiles.reload_values")]);
  reloadBtn.addEventListener("click", async () => {
    for (const m of monitors) {
      profile.monitors[m.id] = await snapshotMonitor(m);
    }
    paint(host, all, monitors, templates);
  });
  card.appendChild(reloadBtn);

  const del = el("button", { type: "button", className: "danger" }, [t("profiles.delete")]);
  del.addEventListener("click", () => {
    all.items = all.items.filter((p) => p.id !== profile.id);
    paint(host, all, monitors, templates);
  });
  card.appendChild(del);

  return card;
}

function monitorOverride(profile: Profile, m: MonitorRow): HTMLElement {
  const block = el("div", { className: "profile-monitor card" });
  const head = el("div", { className: "row" });
  const enabled = el("input", {
    type: "checkbox",
    checked: profile.monitors[m.id] !== undefined,
  }) as HTMLInputElement;
  const label = el("label", {}, [m.name || m.id]);
  head.appendChild(label);
  const enableWrap = el("div");
  enableWrap.appendChild(enabled);
  enableWrap.appendChild(
    document.createTextNode(" " + t("profiles.monitor.enable")),
  );
  head.appendChild(enableWrap);
  block.appendChild(head);

  const body = el("div");
  block.appendChild(body);

  const ensure = (): ProfileMonitorSettings => {
    if (!profile.monitors[m.id]) {
      profile.monitors[m.id] = {
        brightness: m.percent === null ? 80 : Math.round(m.percent),
        contrast: null,
        color_preset: null,
      };
    }
    return profile.monitors[m.id];
  };

  function renderBody() {
    body.replaceChildren();
    if (!enabled.checked) return;
    const o = ensure();

    body.appendChild(
      attrSlider(
        t("profiles.attr.brightness"),
        o.brightness,
        0,
        100,
        "%",
        (v) => {
          o.brightness = v;
        },
        () => {
          o.brightness = null;
        },
      ),
    );

    if (m.kind === "external") {
      body.appendChild(
        attrSlider(
          t("profiles.attr.contrast"),
          o.contrast,
          0,
          100,
          "%",
          (v) => {
            o.contrast = v;
          },
          () => {
            o.contrast = null;
          },
        ),
      );
      body.appendChild(
        attrPreset(
          t("profiles.attr.color_preset"),
          o.color_preset,
          (v) => {
            o.color_preset = v;
          },
          () => {
            o.color_preset = null;
          },
        ),
      );
    }
  }

  enabled.addEventListener("change", () => {
    if (enabled.checked) {
      ensure();
    } else {
      delete profile.monitors[m.id];
    }
    renderBody();
  });

  renderBody();
  return block;
}

function attrSlider(
  label: string,
  initial: number | null,
  min: number,
  max: number,
  suffix: string,
  set: (v: number) => void,
  clear: () => void,
): HTMLElement {
  const row = el("div", { className: "row attr-row" });
  const enabled = el("input", {
    type: "checkbox",
    checked: initial !== null,
  }) as HTMLInputElement;
  const cap = el("label", {}, [label]);
  const wrap = el("div", { className: "attr-controls" });
  wrap.appendChild(enabled);
  wrap.appendChild(cap);
  row.appendChild(wrap);

  const slider = el("input", {
    type: "range",
    min: String(min),
    max: String(max),
    step: "1",
    value: String(initial ?? Math.round((min + max) / 2)),
  }) as HTMLInputElement;
  const readout = el("span", { className: "value" }, [
    initial === null ? "—" : `${initial}${suffix}`,
  ]);
  slider.disabled = initial === null;
  slider.addEventListener("input", () => {
    const v = Math.round(Number(slider.value));
    readout.textContent = `${v}${suffix}`;
    set(v);
  });
  enabled.addEventListener("change", () => {
    if (enabled.checked) {
      const v = Math.round(Number(slider.value));
      readout.textContent = `${v}${suffix}`;
      slider.disabled = false;
      set(v);
    } else {
      readout.textContent = "—";
      slider.disabled = true;
      clear();
    }
  });
  const sliderRow = el("div", { className: "slider-row" }, [slider, readout]);
  row.appendChild(sliderRow);
  return row;
}

function attrPreset(
  label: string,
  initial: number | null,
  set: (v: number) => void,
  clear: () => void,
): HTMLElement {
  const row = el("div", { className: "row attr-row" });
  const enabled = el("input", {
    type: "checkbox",
    checked: initial !== null,
  }) as HTMLInputElement;
  const cap = el("label", {}, [label]);
  const wrap = el("div", { className: "attr-controls" });
  wrap.appendChild(enabled);
  wrap.appendChild(cap);
  row.appendChild(wrap);

  const select = el("select", {}) as HTMLSelectElement;
  for (const o of colorPresetOptions()) {
    const opt = el("option", { value: String(o.value) }, [o.label]);
    select.appendChild(opt);
  }
  if (initial !== null) select.value = String(initial);
  select.disabled = initial === null;
  select.addEventListener("change", () => {
    set(Number(select.value));
  });
  enabled.addEventListener("change", () => {
    if (enabled.checked) {
      select.disabled = false;
      set(Number(select.value));
    } else {
      select.disabled = true;
      clear();
    }
  });
  row.appendChild(select);
  return row;
}

function cryptoRandomId(): string {
  const arr = new Uint8Array(8);
  crypto.getRandomValues(arr);
  return Array.from(arr, (b) => b.toString(16).padStart(2, "0")).join("");
}
