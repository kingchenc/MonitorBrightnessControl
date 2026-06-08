/**
 * Tiny UI helpers shared across views.
 */

export function setStatus(text: string) {
  const el = document.getElementById("status");
  if (el) {
    el.textContent = text;
    el.dataset.dynamic = "1";
  }
}

let scanning = false;

export function setScanning(on: boolean) {
  scanning = on;
  const btn = document.getElementById("refresh-btn") as HTMLButtonElement | null;
  if (btn) btn.disabled = on;
}

export function isScanning(): boolean {
  return scanning;
}

export function mountTabs(onChange: (tab: string) => void) {
  const tabs = document.querySelectorAll<HTMLButtonElement>("nav .tab");
  for (const t of tabs) {
    t.addEventListener("click", () => {
      tabs.forEach((b) => b.classList.remove("active"));
      t.classList.add("active");
      document
        .querySelectorAll<HTMLElement>(".tab-panel")
        .forEach((p) => p.classList.remove("active"));
      const panel = document.getElementById(`tab-${t.dataset.tab}`);
      if (panel) panel.classList.add("active");
      onChange(t.dataset.tab ?? "");
    });
  }
}

export function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Partial<HTMLElementTagNameMap[K]> = {},
  children: (Node | string)[] = [],
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  Object.assign(node, attrs);
  for (const c of children) {
    if (typeof c === "string") node.appendChild(document.createTextNode(c));
    else node.appendChild(c);
  }
  return node;
}

/**
 * Persisted collapsed/expanded state for a group of cards, backed by a single
 * localStorage key holding the list of collapsed ids.
 */
export interface CollapseStore {
  has(id: string): boolean;
  set(id: string, collapsed: boolean): void;
}

export function collapsedStore(storageKey: string): CollapseStore {
  const read = (): Set<string> => {
    try {
      const raw = localStorage.getItem(storageKey);
      const arr = raw ? JSON.parse(raw) : [];
      return new Set(Array.isArray(arr) ? (arr as string[]) : []);
    } catch {
      return new Set();
    }
  };
  return {
    has: (id) => read().has(id),
    set: (id, collapsed) => {
      const s = read();
      if (collapsed) s.add(id);
      else s.delete(id);
      try {
        localStorage.setItem(storageKey, JSON.stringify([...s]));
      } catch {
        // localStorage unavailable — state just won't persist.
      }
    },
  };
}

/**
 * Turn a `.card` whose first child is its heading into a collapsible card:
 * the heading becomes a clickable header (with a chevron) that toggles the rest
 * of the card. The collapsed state is read from / written to `store` under
 * `id`. Returns the same card element for chaining.
 */
export function makeCollapsibleCard(
  card: HTMLElement,
  id: string,
  store: CollapseStore,
): HTMLElement {
  const heading = card.firstElementChild as HTMLElement | null;
  if (!heading) return card;

  // Move everything after the heading into a body wrapper.
  const body = el("div", { className: "card-body" });
  let next = heading.nextSibling;
  while (next) {
    const after = next.nextSibling;
    body.appendChild(next);
    next = after;
  }

  // Replace the heading with a header button that wraps a chevron + heading.
  const chevron = el("span", { className: "chevron" }, []);
  const header = el(
    "button",
    { type: "button", className: "collapse-header" },
    [chevron],
  ) as HTMLButtonElement;
  card.replaceChild(header, heading);
  header.appendChild(heading);
  card.appendChild(body);

  const apply = (collapsed: boolean) => {
    card.classList.toggle("collapsed", collapsed);
    body.style.display = collapsed ? "none" : "";
    chevron.textContent = collapsed ? "▸" : "▾"; // ▸ collapsed / ▾ expanded
    header.setAttribute("aria-expanded", String(!collapsed));
  };
  header.addEventListener("click", () => {
    const collapsed = !card.classList.contains("collapsed");
    apply(collapsed);
    store.set(id, collapsed);
  });
  apply(store.has(id));
  return card;
}

/**
 * Debounce: returns a function that waits `delay` ms after the last call
 * before invoking `fn`. Used to throttle slider scrubs.
 */
export function debounce<T extends (...args: never[]) => void>(
  fn: T,
  delay: number,
): T {
  let t: number | undefined;
  return ((...args: Parameters<T>) => {
    if (t !== undefined) clearTimeout(t);
    t = window.setTimeout(() => fn(...args), delay);
  }) as T;
}
