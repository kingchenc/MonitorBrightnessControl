import { getVersion } from "@tauri-apps/api/app";
import { el } from "../ui";
import { t } from "../i18n";

export async function renderAbout(host: HTMLElement) {
  host.replaceChildren();
  host.appendChild(el("h2", {}, [t("app.title")]));

  // Version line, populated asynchronously from Tauri so it always matches
  // the binary that's actually running.
  const versionLine = el("p", { className: "muted small version-line" }, [
    `${t("about.version")} …`,
  ]);
  host.appendChild(versionLine);
  getVersion()
    .then((v) => {
      versionLine.textContent = `${t("about.version")} ${v}`;
    })
    .catch(() => {
      versionLine.remove();
    });

  host.appendChild(el("p", {}, [t("about.tagline")]));
  host.appendChild(
    el("ul", {}, [
      el("li", {}, [t("about.bullet1")]),
      el("li", {}, [t("about.bullet2")]),
      el("li", {}, [t("about.bullet3")]),
    ]),
  );
  host.appendChild(el("p", { className: "muted small" }, [t("about.license")]));
  host.appendChild(
    el("p", { className: "disclaimer" }, [t("about.disclaimer")]),
  );
}
