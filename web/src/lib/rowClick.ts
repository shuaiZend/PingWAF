/**
 * Row-click arbitration for tables whose rows navigate to an edit view.
 *
 * `<tr onClick>` receives the events of every control rendered inside it, so a
 * delete button that only opens a confirmation dialog also triggered the row
 * handler — the page navigated away and the dialog unmounted before it could be
 * answered ("clicking delete opens the edit page"). The guard below keeps the
 * row shortcut for plain cells while leaving row controls to themselves.
 *
 * `[data-row-click="ignore"]` lets a cell opt out explicitly for cases the
 * selector cannot infer (e.g. a custom clickable div that is not a button).
 */
const INTERACTIVE_SELECTOR = [
  "button",
  "a",
  "input",
  "select",
  "textarea",
  "label",
  "summary",
  '[role="button"]',
  '[role="switch"]',
  '[role="checkbox"]',
  '[role="menuitem"]',
  '[data-row-click="ignore"]',
].join(",");

interface ClosestLike {
  closest(selector: string): unknown;
}

/**
 * Whether the row's own click handler should ignore this event target.
 *
 * Accepts any event target: text nodes and synthetic objects without
 * `closest` fall through to `false`, so the row stays clickable everywhere a
 * browser cannot prove an interactive ancestor.
 */
export function shouldIgnoreRowClick(target: EventTarget | null | undefined): boolean {
  if (!target) return false;
  const candidate = target as Partial<ClosestLike>;
  if (typeof candidate.closest !== "function") return false;
  return candidate.closest(INTERACTIVE_SELECTOR) != null;
}

export { INTERACTIVE_SELECTOR };
