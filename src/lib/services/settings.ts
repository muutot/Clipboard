// Settings service entry point: the reactive `generalSettings` store, the general
// settings IPC calls, and the re-exports every caller imports from
// `$lib/services/settings`. Field validation, normalization, browser storage,
// and window config live in `./settings/*`.

import {
  applySettingsPatch,
  diffSettings,
  mergeSettingsPatches,
  type SettingsPatch,
} from "$lib/utils/settings/settings-patch";
import { listen } from "@tauri-apps/api/event";
import { get, writable } from "svelte/store";
import { setLocale } from "$lib/i18n";
import { invokeTauri, isTauriRuntime } from "$lib/services/runtime";
import type { GeneralSettings, GeneralSettingsInfo } from "$lib/types/clipboard";

import { normalizeGeneralSettings } from "./settings/normalize";
import {
  LOCALE_STORAGE_KEY,
  parseStorageObject,
  readLegacySettings,
  removeStorage,
  saveBrowserSettings,
  STORAGE_KEY,
} from "./settings/storage";
import { cloneDefaults, isRecord } from "./settings/validators";

export { DEFAULT_GENERAL_SETTINGS } from "./settings/defaults";
export { validHexColor } from "./settings/validators";
export {
  DEFAULT_WINDOW_CONFIG,
  getWindowConfig,
  restoreWindowPosition,
  saveWindowPosition,
  setWindowConfig,
  windowConfig,
} from "./settings/window";

const PERSIST_DEBOUNCE_MS = 120;

export async function getGeneralSettings(): Promise<GeneralSettingsInfo> {
  return invokeTauri<GeneralSettingsInfo>("get_general_settings", undefined, {
    settings: cloneDefaults(),
    legacyMigrationRequired: false,
  });
}

export async function setGeneralSettings(value: GeneralSettings): Promise<GeneralSettings> {
  const settings = normalizeGeneralSettings(value);
  return invokeTauri<GeneralSettings>("set_general_settings", { settings }, settings);
}

function createSettingsStore() {
  const desktop = isTauriRuntime();
  const browserInitial = normalizeGeneralSettings(parseStorageObject(STORAGE_KEY), cloneDefaults());
  const store = writable<GeneralSettings>(desktop ? cloneDefaults() : browserInitial);
  let applyingExternalValue = false;
  let initialized = !desktop;
  let initialization: Promise<void> | undefined;
  // Keep intent separately from the displayed snapshot, including before hydration.
  let pendingPatch: SettingsPatch | undefined;
  let writeTimer: ReturnType<typeof setTimeout> | undefined;
  let writeInFlight: Promise<void> | undefined;
  let unlistenSettings: (() => void) | undefined;
  let legacyMigrationPending = false;
  let refreshAfterWrite = false;

  function applyRemote(value: unknown): void {
    const remote = normalizeGeneralSettings(value, cloneDefaults());
    const normalized = pendingPatch
      ? normalizeGeneralSettings(applySettingsPatch(remote, pendingPatch), cloneDefaults())
      : remote;
    store.set(normalized);
    setLocale(normalized.language);
  }

  if (!desktop && typeof window !== "undefined") {
    store.subscribe((value) => {
      if (!applyingExternalValue) saveBrowserSettings(value);
    });
    window.addEventListener("storage", (event) => {
      if (event.key !== STORAGE_KEY || !event.newValue) return;
      try {
        applyingExternalValue = true;
        const normalized = normalizeGeneralSettings(JSON.parse(event.newValue), get(store));
        store.set(normalized);
        setLocale(normalized.language);
      } catch {
        // Ignore malformed values from another browser tab.
      } finally {
        applyingExternalValue = false;
      }
    });
  }

  function clearWriteTimer(): void {
    if (writeTimer !== undefined) clearTimeout(writeTimer);
    writeTimer = undefined;
  }

  function schedulePersist(): void {
    if (!desktop || !initialized) return;
    clearWriteTimer();
    writeTimer = setTimeout(() => {
      writeTimer = undefined;
      void drainWrites().catch((err) => console.error("Settings persist failed:", err));
    }, PERSIST_DEBOUNCE_MS);
  }

  function drainWrites(): Promise<void> {
    if (!desktop) return Promise.resolve();
    if (writeInFlight) return writeInFlight;

    writeInFlight = (async () => {
      while (pendingPatch || refreshAfterWrite) {
        if (pendingPatch) {
          const patch = pendingPatch;
          pendingPatch = undefined;
          clearWriteTimer();
          try {
            const saved = await invokeTauri<GeneralSettings>(
              "set_general_settings",
              { patch },
              get(store),
            );
            applyRemote(saved);
            if (legacyMigrationPending) {
              removeStorage(STORAGE_KEY);
              removeStorage(LOCALE_STORAGE_KEY);
              legacyMigrationPending = false;
            }
          } catch (error) {
            // Retry failed fields underneath newer edits, never an old snapshot.
            pendingPatch = mergeSettingsPatches(patch, pendingPatch ?? {});
            clearWriteTimer();
            throw error;
          }
        } else {
          refreshAfterWrite = false;
          try {
            const response = await getGeneralSettings();
            applyRemote(response.settings);
          } catch (error) {
            console.error("Settings refresh after write failed:", error);
            // Keep the refresh for the next flush/edit, without an automatic retry loop.
            refreshAfterWrite = true;
            if (!pendingPatch) break;
          }
          // An edit/event arriving during hydration is handled by this same drain.
        }
      }
    })().finally(() => {
      writeInFlight = undefined;
    });
    return writeInFlight;
  }

  async function initialize(): Promise<void> {
    if (initialization) return initialization;
    initialization = (async () => {
      if (!desktop) return;
      try {
        unlistenSettings = await listen<GeneralSettings>("general-settings-changed", (event) => {
          if (!initialized || writeInFlight || pendingPatch) {
            refreshAfterWrite = true;
            return;
          }
          applyRemote(event.payload);
        });
      } catch {
        // A missing event permission must not prevent settings persistence.
      }

      let response: GeneralSettingsInfo;
      try {
        response = await getGeneralSettings();
      } catch {
        initialized = true;
        if (pendingPatch) schedulePersist();
        return;
      }

      const remote = normalizeGeneralSettings(response.settings, cloneDefaults());
      if (response.legacyMigrationRequired) {
        legacyMigrationPending = true;
        const legacy = readLegacySettings();
        const migrated = normalizeGeneralSettings(legacy.settings, remote);
        const legacyRecord = isRecord(legacy.settings) ? legacy.settings : {};
        if (legacy.locale && legacyRecord.language !== "zh-CN" && legacyRecord.language !== "en") {
          migrated.language = legacy.locale;
        }
        pendingPatch = mergeSettingsPatches(diffSettings(remote, migrated), pendingPatch ?? {});
      } else {
        removeStorage(STORAGE_KEY);
        removeStorage(LOCALE_STORAGE_KEY);
      }

      applyRemote(remote);
      initialized = true;
      if (legacyMigrationPending) {
        // Preserve legacy keys on failure; later flush/edit retries the same patch.
        await drainWrites().catch((error) => console.error("Settings migration failed:", error));
      } else if (pendingPatch || refreshAfterWrite) {
        schedulePersist();
      }
    })();
    return initialization;
  }

  function merge(partial: Partial<GeneralSettings>) {
    const current = get(store);
    const next = normalizeGeneralSettings({ ...current, ...partial }, current);
    const patch = diffSettings(current, next);
    if (desktop && Object.keys(patch).length) {
      pendingPatch = mergeSettingsPatches(pendingPatch ?? {}, patch);
    }
    store.set(next);
    if (pendingPatch) schedulePersist();
  }

  function updateSetting<K extends keyof GeneralSettings>(key: K, value: GeneralSettings[K]) {
    merge({ [key]: value });
  }

  async function flush(): Promise<void> {
    if (!desktop) return;
    if (!initialized && initialization) await initialization;
    clearWriteTimer();
    do {
      await drainWrites();
      // Include edits queued just as the previous drain's promise settled.
    } while (pendingPatch);
  }

  void initialize();

  return {
    ...store,
    updateSetting,
    merge,
    initialize,
    flush,
    /** Exposed for lifecycle cleanup in tests and future window teardown. */
    destroy() {
      clearWriteTimer();
      if (pendingPatch) void flush().catch((err) => console.error("Settings persist failed:", err));
      unlistenSettings?.();
      unlistenSettings = undefined;
    },
  };
}

export const generalSettings = createSettingsStore();
