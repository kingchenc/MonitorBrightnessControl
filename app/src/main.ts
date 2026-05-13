import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { renderMonitors } from "./views/monitors";
import { renderSettings } from "./views/settings";
import { renderProfiles } from "./views/profiles";
import { renderAbout } from "./views/about";
import { setStatus, setScanning, mountTabs } from "./ui";
import { initLocale, t } from "./i18n";

const TABS: { id: string; key: string }[] = [
  { id: "monitors", key: "tab.monitors" },
  { id: "settings", key: "tab.settings" },
  { id: "profiles", key: "tab.profiles" },
  { id: "about", key: "tab.about" },
];

function renderActive(id: string) {
  const panel = document.getElementById(`tab-${id}`);
  if (!panel) return;
  switch (id) {
    case "monitors":
      renderMonitors(panel);
      break;
    case "settings":
      renderSettings(panel, () => applyChrome());
      break;
    case "profiles":
      renderProfiles(panel);
      break;
    case "about":
      renderAbout(panel);
      break;
  }
}

function activeTab(): string {
  return (
    (document.querySelector("nav .tab.active") as HTMLElement | null)?.dataset.tab ?? "monitors"
  );
}

/**
 * Update language-bound chrome (title, tabs, footer button) so every
 * locale change reflects without a full page reload.
 */
function applyChrome() {
  document.documentElement.lang = (document.documentElement.lang || "en").slice(0, 2);
  document.title = t("app.title");
  const titleEl = document.getElementById("app-title");
  if (titleEl) titleEl.textContent = t("app.title");

  const refreshBtn = document.getElementById("refresh-btn");
  if (refreshBtn) refreshBtn.textContent = t("footer.refresh");

  const status = document.getElementById("status");
  if (status && (status.textContent === "" || status.dataset.dynamic !== "1")) {
    status.textContent = t("status.ready");
  }

  const nav = document.getElementById("tab-nav");
  if (!nav) return;
  const previouslyActive = activeTab();
  nav.replaceChildren();
  for (const tab of TABS) {
    const btn = document.createElement("button");
    btn.className = "tab" + (tab.id === previouslyActive ? " active" : "");
    btn.type = "button";
    btn.dataset.tab = tab.id;
    btn.textContent = t(tab.key);
    nav.appendChild(btn);
  }
  // Re-attach handlers — `mountTabs` reads the DOM each time.
  mountTabs((id) => renderActive(id));
}

async function loadInitialLocale() {
  let saved = "auto";
  try {
    const settings = await invoke<{ language?: string }>("load_settings");
    if (settings && typeof settings.language === "string") saved = settings.language;
  } catch {
    // first launch / IPC race — fall through to default
  }
  await initLocale(saved);
}

async function main() {
  await loadInitialLocale();
  applyChrome();

  document.getElementById("refresh-btn")?.addEventListener("click", async () => {
    setScanning(true);
    setStatus(t("status.refreshing"));
    try {
      // Fire-and-forget — the backend emits `monitors-changed` when work
      // finishes, so the UI is not held by the slow DDC/CI roundtrips.
      await invoke("trigger_refresh");
    } catch (e) {
      setScanning(false);
      setStatus(`${t("common.error")}: ${e}`);
    }
  });

  // Backend scan lifecycle: disable Refresh and show a scan-in-progress
  // status while monitors are being enumerated.
  await listen<boolean>("scan-state", (e) => {
    const on = e.payload === true;
    setScanning(on);
    if (on) {
      setStatus(t("status.scanning"));
    } else {
      setStatus(t("status.updated"));
    }
    renderActive(activeTab());
  });

  // Re-render whichever tab is active each time the backend reports a
  // monitor change (initial enumeration finished, manual refresh, etc.).
  await listen("monitors-changed", () => {
    renderActive(activeTab());
  });

  // Assume an initial scan is in flight: the backend kicks one off in its
  // setup hook. The first `scan-state false` event will clear this.
  setScanning(true);
  setStatus(t("status.scanning"));
  renderActive("monitors");
}

main().catch((e) => {
  setStatus(`Fatal: ${e}`);
  console.error(e);
});
