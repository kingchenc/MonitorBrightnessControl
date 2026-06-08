import { invoke } from "@tauri-apps/api/core";
import { debounce, el, isScanning, setStatus } from "../ui";
import { t } from "../i18n";

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

// Client-side cache of VCP reads (contrast 0x12, colour preset 0x14, …) keyed
// by `${monitorId}:${code}`. DDC/CI reads take seconds, so once a value is
// known we render from the cache on every re-render instead of flashing the
// default placeholder and re-reading the hardware each time. The cache is kept
// up to date when the user changes a value, and is cleared on an explicit
// Refresh so external changes are picked up.
const vcpCache = new Map<string, VcpView>();
const vcpUnsupported = new Set<string>();

function vcpKey(id: string, code: number): string {
  return `${id}:${code}`;
}

/** Drop all cached VCP reads so the next render re-queries the hardware. */
export function clearVcpCache() {
  vcpCache.clear();
  vcpUnsupported.clear();
}

export async function renderMonitors(host: HTMLElement) {
  host.replaceChildren();
  host.appendChild(el("p", { className: "muted" }, [t("monitors.loading")]));
  let rows: MonitorRow[];
  try {
    rows = await invoke<MonitorRow[]>("list_monitors");
  } catch (e) {
    host.replaceChildren(el("p", { className: "error" }, [`${t("common.error")}: ${e}`]));
    return;
  }
  host.replaceChildren();
  if (rows.length === 0) {
    const msg = isScanning() ? t("status.scanning") : t("monitors.empty");
    host.appendChild(el("p", { className: "muted" }, [msg]));
    return;
  }
  for (const row of rows) {
    host.appendChild(monitorCard(row));
  }
}

function monitorCard(row: MonitorRow): HTMLElement {
  const card = el("article", { className: `card kind-${row.kind}` });
  const title = el("header");
  title.appendChild(el("h2", {}, [row.name]));
  title.appendChild(el("span", { className: "kind-badge" }, [row.kind]));
  card.appendChild(title);

  const slider = el("input", {
    type: "range",
    min: "0",
    max: "100",
    step: "1",
    value: String(Math.round(row.percent ?? 50)),
    className: "brightness-slider",
  }) as HTMLInputElement;
  const valueLabel = el("output", { className: "value" }, [
    row.percent === null ? "—" : `${Math.round(row.percent)}%`,
  ]);

  const apply = debounce(async (value: number) => {
    try {
      await invoke("set_brightness", { id: row.id, percent: value });
      setStatus(`${row.name}: ${value}%`);
    } catch (e) {
      setStatus(`${row.name}: ${e}`);
    }
  }, 50);

  slider.addEventListener("input", () => {
    const v = Number(slider.value);
    valueLabel.textContent = `${v}%`;
    apply(v);
  });

  const sliderRow = el("div", { className: "slider-row" }, [slider, valueLabel]);
  card.appendChild(sliderRow);

  if (row.kind === "external") {
    card.appendChild(externalControls(row.id));
  }
  return card;
}

function externalControls(id: string): HTMLElement {
  const wrap = el("div", { className: "vcp-controls" });

  wrap.appendChild(vcpSlider(id, 0x12, t("monitors.contrast")));
  const colorPresets: { label: string; value: number }[] = [
    { label: t("monitors.preset.srgb"), value: 1 },
    { label: t("monitors.preset.native"), value: 2 },
    { label: t("monitors.preset.5000k"), value: 4 },
    { label: t("monitors.preset.6500k"), value: 5 },
    { label: t("monitors.preset.7500k"), value: 6 },
    { label: t("monitors.preset.9300k"), value: 8 },
    { label: t("monitors.preset.user"), value: 11 },
  ];
  wrap.appendChild(vcpEnum(id, 0x14, t("monitors.color_preset"), colorPresets));
  return wrap;
}

function vcpSlider(id: string, code: number, label: string): HTMLElement {
  const wrap = el("div", { className: "vcp-row" });
  wrap.appendChild(el("label", {}, [label]));
  const slider = el("input", {
    type: "range",
    min: "0",
    max: "100",
    step: "1",
    value: "50",
  }) as HTMLInputElement;
  const valueLabel = el("output", { className: "value" }, ["—"]);
  wrap.appendChild(slider);
  wrap.appendChild(valueLabel);

  const key = vcpKey(id, code);
  const render = (v: VcpView) => {
    if (v.maximum > 0) {
      slider.max = String(v.maximum);
      slider.value = String(v.current);
      valueLabel.textContent = `${Math.round((v.current / v.maximum) * 100)}%`;
      slider.disabled = false;
    }
  };

  const cached = vcpCache.get(key);
  if (cached) {
    render(cached);
  } else if (vcpUnsupported.has(key)) {
    slider.disabled = true;
    valueLabel.textContent = "n/a";
  } else {
    // First time we see this control — read the hardware once, then cache it.
    invoke<VcpView>("get_vcp", { id, code })
      .then((v) => {
        vcpCache.set(key, v);
        // Don't clobber the user if they grabbed the slider in the meantime.
        if (document.activeElement !== slider) render(v);
      })
      .catch(() => {
        vcpUnsupported.add(key);
        slider.disabled = true;
        valueLabel.textContent = "n/a";
      });
  }

  const apply = debounce(async (value: number) => {
    try {
      await invoke("set_vcp", { id, code, value });
      const max = Number(slider.max);
      vcpCache.set(key, { current: value, maximum: max });
      valueLabel.textContent =
        max > 0 ? `${Math.round((value / max) * 100)}%` : `${value}`;
    } catch (e) {
      setStatus(`${label}: ${e}`);
    }
  }, 80);

  slider.addEventListener("input", () => {
    apply(Number(slider.value));
  });
  return wrap;
}

function vcpEnum(
  id: string,
  code: number,
  label: string,
  values: { label: string; value: number }[],
): HTMLElement {
  const wrap = el("div", { className: "vcp-row" });
  wrap.appendChild(el("label", {}, [label]));
  const select = el("select", {}, []) as HTMLSelectElement;
  for (const v of values) {
    const opt = el("option", { value: String(v.value) }, [v.label]);
    select.appendChild(opt);
  }
  wrap.appendChild(select);

  const key = vcpKey(id, code);
  const cached = vcpCache.get(key);
  if (cached) {
    select.value = String(cached.current);
  } else if (vcpUnsupported.has(key)) {
    select.disabled = true;
  } else {
    invoke<VcpView>("get_vcp", { id, code })
      .then((v) => {
        vcpCache.set(key, v);
        if (document.activeElement !== select) select.value = String(v.current);
      })
      .catch(() => {
        vcpUnsupported.add(key);
        select.disabled = true;
      });
  }

  select.addEventListener("change", async () => {
    try {
      const value = Number(select.value);
      await invoke("set_vcp", { id, code, value });
      const prev = vcpCache.get(key);
      vcpCache.set(key, { current: value, maximum: prev?.maximum ?? 0 });
    } catch (e) {
      setStatus(`${label}: ${e}`);
    }
  });
  return wrap;
}
