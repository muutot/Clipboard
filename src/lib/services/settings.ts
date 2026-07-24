import { writable, get } from "svelte/store";
import type { GeneralSettings } from "$lib/types/clipboard";

const STORAGE_KEY = "generalSettings";

function loadFromStorage(): Partial<GeneralSettings> {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return JSON.parse(raw);
  } catch {}
  return {};
}

function createSettingsStore() {
  const defaults: GeneralSettings = {
    language: "zh-CN",
    fontSize: "normal",
    windowTransparency: 95,
    compactMode: false,
    alwaysOnTop: false,
    useSystemTitleBar: false,
    theme: "dark",
    imageFullscreenMode: "overlay",
  };

  const stored = loadFromStorage();
  const initial = { ...defaults, ...stored };

  const store = writable<GeneralSettings>(initial);

  store.subscribe((value) => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
    } catch {}
  });

  return {
    ...store,
    updateSetting<K extends keyof GeneralSettings>(key: K, value: GeneralSettings[K]) {
      store.update((s) => ({ ...s, [key]: value }));
    },
    merge(partial: Partial<GeneralSettings>) {
      store.update((s) => ({ ...s, ...partial }));
    },
  };
}

export const generalSettings = createSettingsStore();
