import { el } from "../ui";
import { t } from "../i18n";

export function renderAbout(host: HTMLElement) {
  host.replaceChildren();
  host.appendChild(el("h2", {}, [t("app.title")]));
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
