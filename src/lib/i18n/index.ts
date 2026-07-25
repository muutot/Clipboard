import { writable, derived, get } from "svelte/store";
import type { Locale, LocaleDefinition } from "./types";
import zhCN from "./locales/zh-CN";
import en from "./locales/en";

const locales: Record<Locale, LocaleDefinition> = {
  "zh-CN": zhCN,
  en,
};

const STORAGE_KEY = "clipboard-locale";

function isDesktopRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

function detectLocale(): Locale {
  try {
    const stored = localStorage.getItem(STORAGE_KEY);
    if (stored === "zh-CN" || stored === "en") return stored;
  } catch {
    // localStorage unavailable
  }

  if (typeof navigator !== "undefined" && navigator.language?.startsWith("zh")) {
    return "zh-CN";
  }
  return "en";
}

export const locale = writable<Locale>(detectLocale());

export const messages = derived(locale, ($locale) => locales[$locale]);

locale.subscribe(($locale) => {
  if (!isDesktopRuntime()) {
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

export function setLocale(value: Locale): void {
  locale.set(value);
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
      (result, [key, paramValue]) => result.replace(`{${key}}`, String(paramValue)),
      value,
    );
  }

  return value;
}
