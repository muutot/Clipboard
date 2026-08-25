import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import {
  WINDOW_POSITION_LEGACY_KEY,
  createWindowBoundsController,
  readLegacyWindowPosition,
  type WindowBoundsTarget,
} from "./window-bounds";
import type { WindowPosition } from "$lib/types/clipboard";

class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  length = 0;
  clear(): void {
    this.store.clear();
  }
  getItem(key: string): string | null {
    return this.store.get(key) ?? null;
  }
  key(index: number): string | null {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string): void {
    this.store.delete(key);
  }
  setItem(key: string, value: string): void {
    this.store.set(key, value);
  }
}

interface StubWindow extends WindowBoundsTarget {
  position: { x: number; y: number };
  size: { width: number; height: number };
}

function stubWindow(init: { x: number; y: number; width: number; height: number }): StubWindow {
  const window: StubWindow = {
    position: { x: init.x, y: init.y },
    size: { width: init.width, height: init.height },
    async outerPosition() {
      return { ...window.position };
    },
    async outerSize() {
      return { ...window.size };
    },
    async setSize(size: PhysicalSize) {
      window.size = { width: size.width, height: size.height };
    },
    async setPosition(position: PhysicalPosition) {
      window.position = { x: position.x, y: position.y };
    },
  };
  return window;
}

function savedPositions(save: ReturnType<typeof vi.fn>): WindowPosition[] {
  return save.mock.calls.map((call) => call[0] as WindowPosition);
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("readLegacyWindowPosition", () => {
  it("rounds finite x/y pairs", () => {
    const storage = new MemoryStorage();
    storage.setItem(WINDOW_POSITION_LEGACY_KEY, JSON.stringify({ x: 10.4, y: -20.6 }));
    expect(readLegacyWindowPosition(storage)).toEqual({ x: 10, y: -21 });
  });

  it("returns null for malformed, non-finite, or missing payloads", () => {
    const storage = new MemoryStorage();
    expect(readLegacyWindowPosition(storage)).toBeNull();
    expect(readLegacyWindowPosition(null)).toBeNull();

    storage.setItem(WINDOW_POSITION_LEGACY_KEY, "not-json");
    expect(readLegacyWindowPosition(storage)).toBeNull();

    storage.setItem(WINDOW_POSITION_LEGACY_KEY, JSON.stringify({ x: Number.NaN, y: 5 }));
    expect(readLegacyWindowPosition(storage)).toBeNull();

    storage.setItem(WINDOW_POSITION_LEGACY_KEY, JSON.stringify({ x: "1", y: 2 }));
    expect(readLegacyWindowPosition(storage)).toBeNull();
  });
});

describe("createWindowBoundsController", () => {
  function makeOptions(overrides?: {
    appWindow?: WindowBoundsTarget | null;
    remembered?: boolean;
    restoreResult?: WindowPosition | null;
    save?: ReturnType<typeof vi.fn>;
  }) {
    const save = overrides?.save ?? vi.fn().mockResolvedValue(undefined);
    return {
      options: {
        appWindow:
          "appWindow" in (overrides ?? {})
            ? (overrides!.appWindow ?? null)
            : stubWindow({ x: 100, y: 50, width: 730, height: 600 }),
        isRemembered: () => overrides?.remembered ?? true,
        savePosition: save as unknown as (p: WindowPosition) => Promise<void>,
        restorePosition: vi.fn().mockResolvedValue(overrides?.restoreResult ?? null),
        storage: new MemoryStorage(),
      },
      save,
    };
  }

  it("scheduleSave persists debounced bounds after move/resize events", async () => {
    const { options, save } = makeOptions();
    const controller = createWindowBoundsController(options);

    // Debounce collapses bursts into one write of the latest bounds.
    controller.scheduleSave();
    controller.scheduleSave();
    await vi.advanceTimersByTimeAsync(60);
    await vi.runAllTimersAsync();

    const positions = savedPositions(save);
    expect(positions).toHaveLength(1);
    expect(positions[0]).toMatchObject({ x: 100, y: 50 });
    expect(positions[0].width).toBeGreaterThanOrEqual(730);
  });

  it("ignores schedule and flush requests when remembering is off or headless", () => {
    const off = makeOptions({ remembered: false });
    const offController = createWindowBoundsController(off.options);
    offController.scheduleSave();
    void offController.flush();

    const headless = makeOptions({ appWindow: null });
    const headlessController = createWindowBoundsController(headless.options);
    headlessController.scheduleSave();
    void headlessController.flush();

    expect(savedPositions(off.save)).toHaveLength(0);
    expect(headless.save).not.toHaveBeenCalled();
  });

  it("flush captures immediately without waiting for the debounce timer", async () => {
    const { options, save } = makeOptions();
    const controller = createWindowBoundsController(options);
    controller.scheduleSave();
    await controller.flush();

    expect(save).toHaveBeenCalledTimes(1);
    expect(savedPositions(save)[0]).toMatchObject({ x: 100, y: 50 });
  });

  it("restore applies saved bounds, clamps the width, and clears the legacy key", async () => {
    const { options, save } = makeOptions({
      restoreResult: { x: 12, y: 34, width: 500, height: 480 },
    });
    options.storage.setItem(WINDOW_POSITION_LEGACY_KEY, JSON.stringify({ x: 999, y: 999 }));
    const target = options.appWindow as StubWindow;
    const controller = createWindowBoundsController(options);

    await controller.restore();

    // Width was clamped up to the minimum before applying.
    expect(target.size).toEqual({ width: 710, height: 480 });
    expect(target.position).toEqual({ x: 12, y: 34 });
    expect(savedPositions(save)).toContainEqual({
      x: 12,
      y: 34,
      width: 710,
      height: 480,
    });
    expect(options.storage.getItem(WINDOW_POSITION_LEGACY_KEY)).toBeNull();
  });

  it("restore migrates a legacy x/y payload using the current native size", async () => {
    const { options, save } = makeOptions({ restoreResult: null });
    options.storage.setItem(WINDOW_POSITION_LEGACY_KEY, JSON.stringify({ x: 40, y: 60 }));
    const controller = createWindowBoundsController(options);

    await controller.restore();

    const migrated = savedPositions(save)[0];
    expect(migrated).toMatchObject({ x: 40, y: 60 });
    expect(migrated.width).toBe(730);
    expect(migrated.height).toBe(600);
    expect(options.storage.getItem(WINDOW_POSITION_LEGACY_KEY)).toBeNull();
  });

  it("restore retries after a failure but only once per successful session", async () => {
    const failing = makeOptions();
    (failing.options.restorePosition as ReturnType<typeof vi.fn>)
      .mockRejectedValueOnce(new Error("ipc down"))
      .mockResolvedValue(null);
    const failingController = createWindowBoundsController(failing.options);
    await failingController.restore();
    await failingController.restore(); // latch reset by the failure
    expect(failing.save).not.toHaveBeenCalled();

    const healthy = makeOptions();
    const healthyController = createWindowBoundsController(healthy.options);
    await healthyController.restore();
    await healthyController.restore(); // second attempt must be suppressed
    expect(healthy.save).not.toHaveBeenCalled();
  });

  it("keeps the failed bounds pending so a later flush retries them", async () => {
    const save = vi
      .fn<(position: WindowPosition) => Promise<void>>()
      .mockRejectedValueOnce(new Error("write failed"))
      .mockResolvedValue(undefined);
    const { options } = makeOptions({ save });
    const controller = createWindowBoundsController(options);

    controller.scheduleSave();
    await vi.advanceTimersByTimeAsync(60); // debounced write fails silently
    expect(save).toHaveBeenCalledTimes(1);

    await controller.flush(); // retries the same pending bounds

    expect(save).toHaveBeenCalledTimes(2);
    expect(savedPositions(save)[1]).toEqual(savedPositions(save)[0]);
  });
});
