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
