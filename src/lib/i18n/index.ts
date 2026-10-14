import { writable, derived, get } from "svelte/store";
import type { Locale, LocaleDefinition } from "./types";
import { isTauriRuntime } from "$lib/services/runtime";
import { getOsLocaleName } from "$lib/services/locale";
import type { Language } from "$lib/types/clipboard";
import zhCN from "./locales/zh-CN";
import en from "./locales/en";

const locales: Record<Locale, LocaleDefinition> = {
  "zh-CN": zhCN,
  en,
};

const STORAGE_KEY = "clipboard-locale";

/**
 * Maps a BCP-47-ish locale name to a supported UI locale. Any `zh` variant
 * (zh-CN, zh-Hans-CN, zh_TW) resolves to `zh-CN`; everything else resolves to
 * `en`. Pure so both the webview and the OS answer share one rule.
 */
export function localeFromName(name: string | null | undefined): Locale {
  return name?.trim().toLowerCase().startsWith("zh") ? "zh-CN" : "en";
}

/** OS UI locale read once per window; `null` until loaded or when unavailable. */
let osLocaleName: string | null = null;
let osLocaleLoaded = false;

/**
 * Reads the OS UI locale from the desktop backend. Must settle before the
 * first render of a window that resolves `language: "system"`, otherwise the
 * webview answer would paint first and the correction would flash. Resolves
 * immediately outside the Tauri runtime.
 */
export async function loadOsLocale(): Promise<void> {
  if (osLocaleLoaded || !isTauriRuntime()) return;
  osLocaleLoaded = true;
  osLocaleName = await getOsLocaleName();
}

/**
 * Follows the system language. The OS answer wins over the webview:
 * `navigator.language` follows the Chromium accepted-language list, which on
 * Windows can report `en-US` while the display language is Chinese.
 */
export function resolveSystemLocale(): Locale {
  const name = osLocaleName ?? (typeof navigator !== "undefined" ? navigator.language : null);
  return localeFromName(name);
}

/** Resolve a settings `Language` (which may be `"system"`) to a concrete locale. */
export function resolveLanguage(value: Language): Locale {
  return value === "system" ? resolveSystemLocale() : value;
}

function detectLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "zh-CN" || stored === "en") return stored;
    if (stored === "system") return resolveSystemLocale();
  } catch {
    // localStorage unavailable
  }

  return resolveSystemLocale();
}

export const locale = writable<Locale>(detectLocale());

export const messages = derived(locale, ($locale) => locales[$locale]);

locale.subscribe(($locale) => {
  if (!isTauriRuntime()) {
    try {
      localStorage.setItem(STORAGE_KEY, $locale);
    } catch {
      // localStorage unavailable
    }
  }
  if (typeof document !== "undefined") {
    document.documentElement.lang = $locale;
  }
});

export function setLocale(value: Language): void {
  locale.set(resolveLanguage(value));
}

export function getLocale(): Locale {
  return get(locale);
}

export function t(path: string, params?: Record<string, string | number>): string {
  return resolvePath(get(messages), path, params);
}

export function resolvePath(
  source: Record<string, any>,
  path: string,
  params?: Record<string, string | number>,
): string {
  const keys = path.split(".");
  let value: unknown = source;

  for (const key of keys) {
    if (value && typeof value === "object" && key in value) {
      value = (value as Record<string, unknown>)[key];
    } else {
      return path;
    }
  }

  if (typeof value !== "string") return path;

  if (params) {
    return Object.entries(params).reduce(
      // The function form prevents `$&`-style replacement patterns inside
      // user-supplied values from being interpreted, and the global flag
      // replaces every occurrence of the placeholder.
      (result, [key, paramValue]) => result.replaceAll(`{${key}}`, () => String(paramValue)),
      value,
    );
  }

  return value;
}
