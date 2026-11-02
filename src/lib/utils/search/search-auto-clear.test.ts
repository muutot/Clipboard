import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createSearchAutoClear } from "./search-auto-clear";

describe("createSearchAutoClear", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  function createHarness(overrides?: { enabled?: boolean; delaySeconds?: number; query?: string }) {
    const onClear = vi.fn();
    const autoClear = createSearchAutoClear({
      enabled: () => overrides?.enabled ?? true,
      delaySeconds: () => overrides?.delaySeconds ?? 5,
      hasQuery: () => Boolean(overrides?.query ?? "term"),
      onClear,
    });
    return { autoClear, onClear };
  }

  it("clears after the configured delay once the window loses focus", () => {
    const { autoClear, onClear } = createHarness({ delaySeconds: 5 });

    autoClear.handleWindowFocus(false);
    expect(onClear).not.toHaveBeenCalled();

    vi.advanceTimersByTime(4_999);
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("does nothing when disabled", () => {
    const { autoClear, onClear } = createHarness({ enabled: false });

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(60_000);
    expect(onClear).not.toHaveBeenCalled();
  });

  it("does nothing when the search box is already empty", () => {
    const { autoClear, onClear } = createHarness({ query: "" });

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(60_000);
    expect(onClear).not.toHaveBeenCalled();
  });

  it("does nothing when the window regains focus", () => {
    const { autoClear, onClear } = createHarness();

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(2_000);
    autoClear.handleWindowFocus(true);
    vi.advanceTimersByTime(60_000);
    expect(onClear).not.toHaveBeenCalled();
  });

  it("restarts the delay when blur fires again", () => {
    const { autoClear, onClear } = createHarness({ delaySeconds: 5 });

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(3_000);
    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(4_999);
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it("skips clearing when focus returned while the timer was pending", () => {
    const { autoClear, onClear } = createHarness();

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(2_000);
    autoClear.handleWindowFocus(true);
    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(1_000);
    autoClear.handleWindowFocus(true);
    vi.advanceTimersByTime(60_000);
    expect(onClear).not.toHaveBeenCalled();
  });

  it("dispose cancels a pending timer", () => {
    const { autoClear, onClear } = createHarness();

    autoClear.handleWindowFocus(false);
    autoClear.dispose();
    vi.advanceTimersByTime(60_000);
    expect(onClear).not.toHaveBeenCalled();
  });

  it("clamps the delay to at least one second", () => {
    const { autoClear, onClear } = createHarness({ delaySeconds: 0 });

    autoClear.handleWindowFocus(false);
    vi.advanceTimersByTime(999);
    expect(onClear).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onClear).toHaveBeenCalledTimes(1);
  });
});
