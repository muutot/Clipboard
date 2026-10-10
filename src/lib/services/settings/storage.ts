// Browser-storage accessors for the localStorage fallback and the legacy
// settings/locale read performed before the Tauri bridge answers.

import type { GeneralSettings, Language } from "$lib/types/clipboard";

export const STORAGE_KEY = "generalSettings";
export const LOCALE_STORAGE_KEY = "clipboard-locale";

function readStorage(key: string): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

export function removeStorage(key: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(key);
  } catch {
    // localStorage may be unavailable in a restricted webview.
  }
}

export function parseStorageObject(key: string): unknown {
  const raw = readStorage(key);
  if (!raw) return undefined;
  try {
    return JSON.parse(raw);
  } catch {
    return undefined;
  }
}

export function readLegacySettings(): { settings: unknown; locale?: Language } {
  const generalRaw = readStorage(STORAGE_KEY);
  const localeRaw = readStorage(LOCALE_STORAGE_KEY);
  const locale = localeRaw === "zh-CN" || localeRaw === "en" ? localeRaw : undefined;
  return {
    settings: parseStorageObject(STORAGE_KEY),
    locale,
  };
}

export function saveBrowserSettings(value: GeneralSettings): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
  } catch {
    // localStorage may be unavailable in a restricted browser context.
  }
}
