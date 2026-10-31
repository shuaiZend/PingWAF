import { describe, expect, it } from "vitest";

import { INTERACTIVE_SELECTOR, shouldIgnoreRowClick } from "../rowClick";

/** Minimal stand-in for a DOM Element: records the selector it was asked for. */
function node(closestResult: object | null): {
  closest: (selector: string) => unknown;
  selectors: string[];
} {
  const selectors: string[] = [];
  return {
    selectors,
    closest(selector: string) {
      selectors.push(selector);
      return closestResult;
    },
  };
}

describe("shouldIgnoreRowClick", () => {
  it("keeps the row shortcut for a plain cell", () => {
    expect(shouldIgnoreRowClick(node(null) as unknown as EventTarget)).toBe(false);
  });

  it("ignores clicks that start inside a control", () => {
    const button = node({ tagName: "BUTTON" });
    expect(shouldIgnoreRowClick(button as unknown as EventTarget)).toBe(true);
    expect(button.selectors[0]).toBe(INTERACTIVE_SELECTOR);
  });

  it("ignores clicks inside an explicit opt-out", () => {
    expect(
      shouldIgnoreRowClick(node({ dataset: { rowClick: "ignore" } }) as unknown as EventTarget),
    ).toBe(true);
  });

  it("covers every control kind a row can contain", () => {
    for (const selector of [
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
    ]) {
      expect(INTERACTIVE_SELECTOR).toContain(selector);
    }
  });

  it("does not crash on targets without closest (text nodes, synthetic events)", () => {
    expect(shouldIgnoreRowClick(null)).toBe(false);
    expect(shouldIgnoreRowClick({} as unknown as EventTarget)).toBe(false);
  });
});
