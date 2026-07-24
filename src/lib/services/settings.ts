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
    fontSizes: { base: 14, secondary: 11, tiny: 10 },
    display: { showSecondaryText: true },
    windowTransparency: 95,
    compactMode: false,
    compactPaddingTop: 6,
    compactPaddingBottom: 4,
    compactCardGap: 5,
    compactTextHeight: 58,
    compactTallTextHeight: 70,
    compactImageHeight: 130,
    compactSearchHeight: 40,
    compactSearchFontSize: 14,
    compactCardBorderRadius: 10,
    pinCopiedToTop: true,
    rememberWindowPosition: false,
    alwaysOnTop: false,
    useSystemTitleBar: false,
    theme: "dark",
    imageFullscreenMode: "overlay",
    viewerBackdropOpacity: 92,
  };

  const stored = loadFromStorage();
  const initial = { ...defaults, ...stored };

  const store = writable<GeneralSettings>(initial);

  store.subscribe((value) => {
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
    } catch {}
  });

  if (typeof window !== "undefined") {
    window.addEventListener("storage", (e) => {
      if (e.key === STORAGE_KEY && e.newValue) {
        try {
          const parsed = JSON.parse(e.newValue) as Partial<GeneralSettings>;
          store.update((s) => ({ ...s, ...parsed }));
        } catch {}
      }
    });
  }

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
