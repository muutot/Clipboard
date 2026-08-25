// Window bounds persistence extracted from the main route so the debounced
// save/drain loop and the legacy x/y migration can be tested without a real
// Tauri window. The route wires the controller to the actual WebviewWindow,
// the `rememberWindowPosition` setting, and the settings service wrappers.

import { PhysicalPosition, PhysicalSize } from "@tauri-apps/api/window";
import type { WindowPosition } from "$lib/types/clipboard";

export const WINDOW_POSITION_LEGACY_KEY = "windowPosition";
export const MIN_WINDOW_WIDTH = 710;

/** Structural subset of Tauri's WebviewWindow used by the controller. */
export interface WindowBoundsTarget {
  outerPosition(): Promise<{ x: number; y: number }>;
  outerSize(): Promise<{ width: number; height: number }>;
  setSize(size: PhysicalSize): Promise<void>;
  setPosition(position: PhysicalPosition): Promise<void>;
}

export function readLegacyWindowPosition(
  storage: Pick<Storage, "getItem"> | null,
): { x: number; y: number } | null {
  try {
    const raw = storage?.getItem(WINDOW_POSITION_LEGACY_KEY) ?? null;
    if (!raw) return null;
    const parsed = JSON.parse(raw) as { x?: unknown; y?: unknown };
    if (
      typeof parsed.x !== "number" ||
      !Number.isFinite(parsed.x) ||
      typeof parsed.y !== "number" ||
      !Number.isFinite(parsed.y)
    ) {
      return null;
    }
    return { x: Math.round(parsed.x), y: Math.round(parsed.y) };
  } catch {
    return null;
  }
}

function removeLegacyPosition(storage: Pick<Storage, "removeItem"> | null): void {
  try {
    storage?.removeItem(WINDOW_POSITION_LEGACY_KEY);
  } catch {}
}

const SAVE_DEBOUNCE_MS = 50;

export interface WindowBoundsControllerOptions {
  appWindow: WindowBoundsTarget | null;
  isRemembered(): boolean;
  savePosition(position: WindowPosition): Promise<void>;
  restorePosition(): Promise<WindowPosition | null>;
  storage?: Pick<Storage, "getItem" | "removeItem"> | null;
}

export interface WindowBoundsController {
  /** Debounced capture of the current bounds after a move/resize. */
  scheduleSave(): void;
  /** Capture immediately and drain pending writes (lifecycle teardown). */
  flush(): Promise<void>;
  /** Restore saved bounds once per remember-window session. */
  restore(): Promise<void>;
  /** Allow another restore attempt when remembering is re-enabled. */
  resetRestoreAttempt(): void;
}

export function createWindowBoundsController(
  options: WindowBoundsControllerOptions,
): WindowBoundsController {
  const { appWindow, isRemembered, savePosition, restorePosition } = options;
  const storage = options.storage ?? null;
  let restoreAttempted = false;
  let saveTimer: ReturnType<typeof setTimeout> | undefined;
  let writeInFlight: Promise<void> | undefined;
  let pendingBounds: WindowPosition | undefined;

  async function captureBounds(): Promise<WindowPosition> {
    if (!appWindow) throw new Error("window bounds are only available in Tauri");
    const [position, size] = await Promise.all([appWindow.outerPosition(), appWindow.outerSize()]);
    return {
      x: position.x,
      y: position.y,
      width: Math.max(size.width, MIN_WINDOW_WIDTH),
      height: size.height,
    };
  }

  function drainWrites(): Promise<void> {
    if (!appWindow) return Promise.resolve();
    if (writeInFlight) return writeInFlight;

    writeInFlight = (async () => {
      while (pendingBounds) {
        if (!isRemembered()) {
          pendingBounds = undefined;
          return;
        }
        const bounds = pendingBounds;
        pendingBounds = undefined;
        try {
          await savePosition(bounds);
        } catch (error) {
          pendingBounds = bounds;
          throw error;
        }
      }
    })().finally(() => {
      writeInFlight = undefined;
    });
    return writeInFlight;
  }

  function scheduleSave(): void {
    if (!appWindow || !isRemembered()) return;
    if (saveTimer !== undefined) clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      saveTimer = undefined;
      void captureBounds()
        .then((bounds) => {
          if (!isRemembered()) return;
          pendingBounds = bounds;
          return drainWrites();
        })
        .catch(() => {});
    }, SAVE_DEBOUNCE_MS);
  }

  async function flush(): Promise<void> {
    if (!appWindow || !isRemembered()) return;
    if (saveTimer !== undefined) {
      clearTimeout(saveTimer);
      saveTimer = undefined;
      try {
        pendingBounds = await captureBounds();
      } catch {
        return;
      }
    }
    await drainWrites().catch(() => {});
  }

  async function restore(): Promise<void> {
    if (!appWindow || !isRemembered() || restoreAttempted) return;
    restoreAttempted = true;
    try {
      const saved = await restorePosition();
      if (!isRemembered()) return;
      if (saved && saved.width > 0 && saved.height > 0) {
        const width = Math.max(saved.width, MIN_WINDOW_WIDTH);
        await appWindow.setSize(new PhysicalSize(width, saved.height));
        await appWindow.setPosition(new PhysicalPosition(saved.x, saved.y));
        if (width !== saved.width) {
          await savePosition({ ...saved, width });
        }
        removeLegacyPosition(storage);
        return;
      }

      // Migrate the old x/y-only browser storage once the backend has no
      // bounds yet. The current native size supplies the missing dimensions.
      const legacy = readLegacyWindowPosition(storage);
      if (!legacy) return;
      const size = await appWindow.outerSize();
      const migrated: WindowPosition = {
        x: legacy.x,
        y: legacy.y,
        width: Math.max(size.width, MIN_WINDOW_WIDTH),
        height: size.height,
      };
      if (!isRemembered()) return;
      await savePosition(migrated);
      if (!isRemembered()) return;
      await appWindow.setPosition(new PhysicalPosition(migrated.x, migrated.y));
      removeLegacyPosition(storage);
    } catch {
      // Keep the legacy key if restoring or migrating failed; retry on the
      // next transition to rememberWindowPosition=true.
      restoreAttempted = false;
    }
  }

  function resetRestoreAttempt(): void {
    restoreAttempted = false;
  }

  return { scheduleSave, flush, restore, resetRestoreAttempt };
}
