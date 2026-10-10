// Window configuration (`conf/conf.json` window keys) and the window-position IPC
// calls, plus the shared `windowConfig` store loaded once at module scope so
// startup/tray toggles are populated before any settings panel mounts.

import { get, writable } from "svelte/store";
import { invokeTauri } from "$lib/services/runtime";
import type { WindowConfig, WindowPosition } from "$lib/types/clipboard";

export const DEFAULT_WINDOW_CONFIG: WindowConfig = {
  launchAtStartup: false,
  closeToTray: true,
  singleInstance: true,
};

export async function getWindowConfig(): Promise<WindowConfig> {
  return invokeTauri<WindowConfig>("get_window_config", undefined, { ...DEFAULT_WINDOW_CONFIG });
}

export async function setWindowConfig(settings: Partial<WindowConfig>): Promise<void> {
  await invokeTauri<void>("set_window_config", {
    launchAtStartup: settings.launchAtStartup ?? null,
    closeToTray: settings.closeToTray ?? null,
    singleInstance: settings.singleInstance ?? null,
  });
}

/**
 * Shared window-config store. Loading once at module scope keeps the
 * startup/tray toggles populated before any settings panel mounts, so the
 * lazy General panel shows the real value on first paint instead of flipping
 * from the default after `get_window_config` resolves.
 */
function createWindowConfigStore() {
  const store = writable<WindowConfig>({ ...DEFAULT_WINDOW_CONFIG });
  let loaded = false;
  let loading: Promise<void> | undefined;

  function ensureLoaded(): Promise<void> {
    if (loaded) return Promise.resolve();
    if (!loading) {
      loading = getWindowConfig()
        .then((config) => {
          store.set(config);
          loaded = true;
        })
        .catch((error) => {
          console.error("Window config load failed:", error);
        })
        .finally(() => {
          loading = undefined;
        });
    }
    return loading;
  }

  async function update(partial: Partial<WindowConfig>): Promise<void> {
    const previous = get(store);
    const next = { ...previous, ...partial };
    store.set(next);
    try {
      await setWindowConfig(partial);
    } catch (error) {
      store.set(previous);
      throw error;
    }
  }

  void ensureLoaded();

  return { ...store, ensureLoaded, update };
}

export const windowConfig = createWindowConfigStore();

export async function restoreWindowPosition(): Promise<WindowPosition | null> {
  return invokeTauri<WindowPosition>("restore_window_position");
}

export async function saveWindowPosition(position: WindowPosition): Promise<void> {
  await invokeTauri<void>("save_window_position", { ...position });
}
