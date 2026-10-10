import { describe, expect, it } from "vitest";
import { resolveSearchInputAction, type SearchInputContext } from "./search-input-actions";

function keyEvent(
  init: Partial<KeyboardEvent> & { key: string; cancelable?: boolean },
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key: init.key,
    ctrlKey: init.ctrlKey ?? false,
    altKey: init.altKey ?? false,
    shiftKey: init.shiftKey ?? false,
    metaKey: init.metaKey ?? false,
    cancelable: init.cancelable ?? true,
    bubbles: true,
  });
  for (const prop of ["isComposing"] as const) {
    if (prop in init && init[prop] !== undefined) {
      Object.defineProperty(event, prop, { value: init[prop] });
    }
  }
  return event;
}

function ctx(overrides: Partial<SearchInputContext> = {}): SearchInputContext {
  return {
    now: 1000,
    lastBackspaceAt: 0,
    query: "hello",
    suggestionsOpen: false,
    optionCount: 0,
    activeOption: null,
    inlineSuggestion: null,
    caretAtEnd: false,
    ...overrides,
  };
}

describe("resolveSearchInputAction — Backspace", () => {
  it("clears the query on a fast second press", () => {
    const resolved = resolveSearchInputAction(
      keyEvent({ key: "Backspace" }),
      ctx({ lastBackspaceAt: 700 }),
    );
    expect(resolved.action).toEqual({ type: "clear-query" });
    expect(resolved.prevent).toBe(true);
    expect(resolved.backspaceAt).toBe(0);
  });

  it("records the timestamp on a slow press and bubbles", () => {
    const resolved = resolveSearchInputAction(
      keyEvent({ key: "Backspace" }),
      ctx({ lastBackspaceAt: 100 }),
    );
    expect(resolved.action).toEqual({ type: "none" });
    expect(resolved.prevent).toBe(false);
    expect(resolved.backspaceAt).toBe(1000);
  });

  it("resets the timestamp for other keys", () => {
    const resolved = resolveSearchInputAction(
      keyEvent({ key: "a" }),
      ctx({ lastBackspaceAt: 900 }),
    );
    expect(resolved.backspaceAt).toBe(0);
  });

  it("preserves the timestamp while composing", () => {
    const event = keyEvent({ key: "a" });
    Object.defineProperty(event, "isComposing", { value: true });
    const resolved = resolveSearchInputAction(event, ctx({ lastBackspaceAt: 900 }));
    expect(resolved.action).toEqual({ type: "none" });
    expect(resolved.backspaceAt).toBe(900);
  });
});

describe("resolveSearchInputAction — suggestions", () => {
  it("accepts the inline suggestion on Tab without Shift", () => {
    const resolved = resolveSearchInputAction(
      keyEvent({ key: "Tab" }),
      ctx({ inlineSuggestion: "hello world", caretAtEnd: true }),
    );
    expect(resolved.action).toEqual({ type: "accept-inline", value: "hello world" });
    expect(resolved.prevent).toBe(true);
    expect(resolved.stop).toBe(true);
  });

  it("ignores Tab with Shift or a mid-text caret", () => {
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "Tab", shiftKey: true }),
        ctx({ inlineSuggestion: "hello world", caretAtEnd: true }),
      ).action.type,
    ).toBe("none");
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "Tab" }),
        ctx({ inlineSuggestion: "hello world", caretAtEnd: false }),
      ).action.type,
    ).toBe("none");
  });

  it("moves the option index with wrapping", () => {
    // Wrapping itself lives in the route; the resolver only reports direction.
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "ArrowDown" }),
        ctx({ suggestionsOpen: true, optionCount: 3 }),
      ),
    ).toMatchObject({ action: { type: "move-index", delta: 1 }, prevent: true, stop: true });
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "ArrowUp" }),
        ctx({ suggestionsOpen: true, optionCount: 3 }),
      ),
    ).toMatchObject({ action: { type: "move-index", delta: -1 } });
    expect(resolveSearchInputAction(keyEvent({ key: "ArrowDown" }), ctx()).action.type).toBe(
      "move-list-selection",
    );
  });

  it("chooses the active option on Enter, else commits", () => {
    expect(
      resolveSearchInputAction(keyEvent({ key: "Enter" }), ctx({ activeOption: "yesterday" })),
    ).toMatchObject({ action: { type: "choose-option", value: "yesterday" } });
    expect(resolveSearchInputAction(keyEvent({ key: "Enter" }), ctx()).action).toEqual({
      type: "commit-query",
    });
  });

  it("closes suggestions and clears the query on Escape", () => {
    expect(
      resolveSearchInputAction(keyEvent({ key: "Escape" }), ctx({ suggestionsOpen: true })),
    ).toMatchObject({ action: { type: "close-suggestions" }, prevent: true, stop: true });
    expect(
      resolveSearchInputAction(keyEvent({ key: "Escape" }), ctx({ query: "" })).action,
    ).toEqual({ type: "none" });
  });
});

describe("resolveSearchInputAction — list navigation", () => {
  it("moves the list selection on Up/Down when there is no hint or listbox", () => {
    expect(resolveSearchInputAction(keyEvent({ key: "ArrowDown" }), ctx())).toMatchObject({
      action: { type: "move-list-selection", delta: 1 },
      prevent: true,
      stop: true,
    });
    expect(resolveSearchInputAction(keyEvent({ key: "ArrowUp" }), ctx())).toMatchObject({
      action: { type: "move-list-selection", delta: -1 },
      prevent: true,
      stop: true,
    });
  });

  it("moves the list selection when the listbox is open but empty", () => {
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "ArrowDown" }),
        ctx({ suggestionsOpen: true, optionCount: 0 }),
      ),
    ).toMatchObject({ action: { type: "move-list-selection", delta: 1 } });
  });

  it("keeps Up/Down for the inline hint, not the list", () => {
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "ArrowDown" }),
        ctx({ inlineSuggestion: "hello world", caretAtEnd: true }),
      ).action.type,
    ).toBe("none");
  });

  it("still moves the suggestion highlight when the listbox has options", () => {
    expect(
      resolveSearchInputAction(
        keyEvent({ key: "ArrowDown" }),
        ctx({ suggestionsOpen: true, optionCount: 2, inlineSuggestion: "hello world" }),
      ),
    ).toMatchObject({ action: { type: "move-index", delta: 1 } });
  });
});
