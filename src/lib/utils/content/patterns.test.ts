import { describe, expect, it } from "vitest";
import { COLOR_RE, detectQuickActions, extractColors, quickActionKind } from "./patterns";
import type { QuickAction } from "$lib/services/clipboard";

function action(overrides: Partial<QuickAction> & { payload: string }): QuickAction {
  return { label: "action", actionType: "open", ...overrides };
}

describe("COLOR_RE", () => {
  it("matches shorthand, 6-digit, and 8-digit HEX colors", () => {
    expect(extractColors("#f00 accent #ff0000 bold #ff000080 alpha")).toEqual([
      "#f00",
      "#ff0000",
      "#ff000080",
    ]);
    // A 7-digit run is not a color and must not be truncated into one.
    expect([..."#abcdefg".matchAll(COLOR_RE)]).toEqual([]);
  });
});

describe("quickActionKind", () => {
  it("prefers the backend-provided kind", () => {
    expect(quickActionKind(action({ payload: "anything", kind: "phone" }))).toBe("phone");
    expect(
      quickActionKind(action({ payload: "https://x", actionType: "open", kind: "color" })),
    ).toBe("color");
  });

  it("derives the kind from the action type and payload", () => {
    expect(quickActionKind(action({ payload: "2026-09-11", actionType: "viewDate" }))).toBe("date");
    expect(quickActionKind(action({ payload: "mailto:a@b.com" }))).toBe("email");
    expect(quickActionKind(action({ payload: "tel:+123" }))).toBe("phone");
    expect(quickActionKind(action({ payload: "https://example.com" }))).toBe("url");
    expect(quickActionKind(action({ payload: "http://example.com" }))).toBe("url");
    expect(quickActionKind(action({ payload: "#ff0000", actionType: "copy" }))).toBe("color");
  });

  it("falls back to copy for unknown payloads", () => {
    expect(quickActionKind(action({ payload: "plain text" }))).toBe("copy");
    expect(quickActionKind(action({ payload: "ftp://example.com" }))).toBe("copy");
    expect(quickActionKind(action({ payload: "#ff0000", actionType: "open" }))).toBe("copy");
    expect(quickActionKind(action({ payload: "#12345", actionType: "copy" }))).toBe("copy");
    expect(quickActionKind(action({ payload: "#1234567", actionType: "copy" }))).toBe("copy");
  });

  it("detects 4/8-digit HEX colors in firstOnly mode", () => {
    const actions = detectQuickActions("paint #ff000080 now", true);
    expect(actions.find((entry) => entry.kind === "color")?.payload).toBe("#ff000080");
  });

  it("skips invalid date candidates instead of abandoning the scan (firstOnly)", () => {
    // 13/45/2024 parses under neither d/m/y reading; findFirstDate must keep
    // scanning and find the valid 15/6/2024 instead of returning nothing.
    const actions = detectQuickActions("13/45/2024 x 15/6/2024", true);
    const dateAction = actions.find((entry) => entry.kind === "date");
    expect(dateAction?.payload).toBe("2024-06-15");
  });

  it("keeps scanning y/m/d matches after an invalid one (firstOnly)", () => {
    // 9999-99-99 matches the y/m/d shape but fails calendar validation;
    // findFirstDate must continue to the valid 2024-05-01.
    const actions = detectQuickActions("9999-99-99 and 2024-05-01", true);
    const dateAction = actions.find((entry) => entry.kind === "date");
    expect(dateAction?.payload).toBe("2024-05-01");
  });

  it("strips trailing sentence punctuation from URLs (firstOnly)", () => {
    // Sentence punctuation glued to the address is not part of it, in both
    // half-width and full-width (CJK) forms.
    for (const [text, expected] of [
      ["visit https://example.com/a, thanks", "https://example.com/a"],
      ["visit https://example.com/a. done", "https://example.com/a"],
      ["visit https://example.com/a。好的", "https://example.com/a"],
      ["visit（https://example.com/a）好吗", "https://example.com/a"],
    ] as const) {
      const actions = detectQuickActions(text, true);
      const urlAction = actions.find((entry) => entry.kind === "url");
      expect(urlAction?.payload).toBe(expected);
    }
    // Legitimate trailing path punctuation survives: dots inside the URL
    // body (e.g. file extensions) are not touched.
    const keep = detectQuickActions("open https://example.com/a.b.c", true);
    expect(keep.find((entry) => entry.kind === "url")?.payload).toBe("https://example.com/a.b.c");
  });
});
