import { describe, expect, it } from "vitest";
import { isEditableKeyboardTarget, shortcutMatchesEvent } from "./keyboard";

function keyEvent(init: Partial<KeyboardEvent> & { key: string }): KeyboardEvent {
  return new KeyboardEvent("keydown", {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    altKey: init.altKey ?? false,
    shiftKey: init.shiftKey ?? false,
    metaKey: init.metaKey ?? false,
    bubbles: true,
  });
}

describe("isEditableKeyboardTarget", () => {
  it("matches input, textarea, and select elements", () => {
    const input = document.createElement("input");
    const textarea = document.createElement("textarea");
    const select = document.createElement("select");
    expect(isEditableKeyboardTarget(input)).toBe(true);
    expect(isEditableKeyboardTarget(textarea)).toBe(true);
    expect(isEditableKeyboardTarget(select)).toBe(true);
  });

  it("matches contenteditable hosts and descendants", () => {
    const host = document.createElement("div");
    host.setAttribute("contenteditable", "true");
    const child = document.createElement("span");
    host.appendChild(child);
    document.body.appendChild(host);

    expect(isEditableKeyboardTarget(host)).toBe(true);
    expect(isEditableKeyboardTarget(child)).toBe(true);
    host.remove();
  });

  it("rejects buttons, plain divs, and null targets", () => {
    const button = document.createElement("button");
    const div = document.createElement("div");
    expect(isEditableKeyboardTarget(button)).toBe(false);
    expect(isEditableKeyboardTarget(div)).toBe(false);
    expect(isEditableKeyboardTarget(null)).toBe(false);
  });

  it("does not treat contenteditable=false as editable", () => {
    const host = document.createElement("div");
    host.setAttribute("contenteditable", "false");
    expect(isEditableKeyboardTarget(host)).toBe(false);
  });
});

describe("shortcutMatchesEvent", () => {
  it("matches single keys case-insensitively across canonical casing", () => {
    expect(shortcutMatchesEvent("V", keyEvent({ key: "v" }))).toBe(true);
    expect(shortcutMatchesEvent("v", keyEvent({ key: "V" }))).toBe(true);
  });

  it("normalizes the Space label against the spacebar key value", () => {
    expect(shortcutMatchesEvent("Space", keyEvent({ key: " " }))).toBe(true);
    expect(shortcutMatchesEvent("space", keyEvent({ key: " " }))).toBe(true);
  });

  it("requires the exact modifier set (Ctrl+V does not match V)", () => {
    expect(shortcutMatchesEvent("Ctrl+V", keyEvent({ key: "v", ctrlKey: true }))).toBe(true);
    expect(shortcutMatchesEvent("Ctrl+V", keyEvent({ key: "v" }))).toBe(false);
    expect(shortcutMatchesEvent("V", keyEvent({ key: "v", ctrlKey: true }))).toBe(false);
    expect(
      shortcutMatchesEvent("Ctrl+Shift+V", keyEvent({ key: "v", ctrlKey: true, shiftKey: true })),
    ).toBe(true);
    // Adding an extra modifier breaks an exact match.
    expect(
      shortcutMatchesEvent(
        "Ctrl+Shift+V",
        keyEvent({ key: "v", ctrlKey: true, shiftKey: true, altKey: true }),
      ),
    ).toBe(false);
  });

  it("accepts alias spellings for modifiers", () => {
    expect(shortcutMatchesEvent("Control+v", keyEvent({ key: "v", ctrlKey: true }))).toBe(true);
    expect(shortcutMatchesEvent("Alt+1", keyEvent({ key: "1", altKey: true }))).toBe(true);
    expect(shortcutMatchesEvent("Option+1", keyEvent({ key: "1", altKey: true }))).toBe(true);
    expect(shortcutMatchesEvent("Cmd+c", keyEvent({ key: "c", metaKey: true }))).toBe(true);
    expect(shortcutMatchesEvent("Meta+c", keyEvent({ key: "c", metaKey: true }))).toBe(true);
  });

  it("matches named navigation keys without case mangling", () => {
    expect(shortcutMatchesEvent("ArrowUp", keyEvent({ key: "ArrowUp" }))).toBe(true);
    expect(shortcutMatchesEvent("Arrowright", keyEvent({ key: "ArrowRight" }))).toBe(true);
    expect(shortcutMatchesEvent("Tab", keyEvent({ key: "Tab" }))).toBe(true);
  });

  it("rejects multi-key or empty canonical strings", () => {
    expect(shortcutMatchesEvent("A+B", keyEvent({ key: "a" }))).toBe(false);
    expect(shortcutMatchesEvent("+", keyEvent({ key: "+" }))).toBe(false);
    expect(shortcutMatchesEvent("", keyEvent({ key: "a" }))).toBe(false);
    expect(shortcutMatchesEvent("Shift+", keyEvent({ key: "a", shiftKey: true }))).toBe(false);
  });
});
