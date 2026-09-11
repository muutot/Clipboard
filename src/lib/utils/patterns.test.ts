import { describe, expect, it } from "vitest";
import { quickActionKind } from "./patterns";
import type { QuickAction } from "$lib/services/clipboard";

function action(overrides: Partial<QuickAction> & { payload: string }): QuickAction {
  return { label: "action", actionType: "open", ...overrides };
}

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
  });
});
