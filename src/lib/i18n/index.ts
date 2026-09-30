import { writable, derived, get } from "svelte/store";
import type { Locale, LocaleDefinition } from "./types";
import { isTauriRuntime } from "$lib/services/runtime";
import type { Language } from "$lib/types/clipboard";
import zhCN from "./locales/zh-CN";
import en from "./locales/en";

const locales: Record<Locale, LocaleDefinition> = {
  "zh-CN": zhCN,
  en,
};

const STORAGE_KEY = "clipboard-locale";

/** Follow the OS locale: Chinese environments get `zh-CN`, everything else `en`. */
export function resolveSystemLocale(): Locale {
  if (typeof navigator !== "undefined" && navigator.language?.startsWith("zh")) {
    return "zh-CN";
  }
  return "en";
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
