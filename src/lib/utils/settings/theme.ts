import type { ThemeColors, ThemeMode } from "#lib/types/clipboard.ts";
import { DARK_THEME_COLORS, LIGHT_THEME_COLORS } from "#lib/types/clipboard.ts";
import { resolveCodeTheme } from "#lib/utils/content/code-theme.ts";

/**
 * Pre-paint theme snapshot. `static/theme-init.js` reads this key from `<head>`
 * before the first paint. System snapshots are re-resolved against the current
 * preference. Keep both cache keys and the variable mapping in
 * sync with that script; the snapshot is only a first-paint hint, never the
 * source of truth.
 */
export const THEME_CACHE_KEY = "clipboard-theme-vars";
export const THEME_MODE_CACHE_KEY = "clipboard-theme-mode";

export function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && typeof window.matchMedia === "function"
    ? window.matchMedia("(prefers-color-scheme: dark)").matches
    : false;
}

export function resolveThemeMode(mode: ThemeMode): Exclude<ThemeMode, "system"> {
  return mode === "system" ? (systemPrefersDark() ? "dark" : "light") : mode;
}

/** Each window owns its media listener and releases it on unmount. */
export function watchSystemTheme(onChange: () => void): () => void {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return () => {};
  const media = window.matchMedia("(prefers-color-scheme: dark)");
  media.addEventListener("change", onChange);
  return () => media.removeEventListener("change", onChange);
}

const COLOR_CSS_MAP: Record<keyof ThemeColors, string> = {
  bg: "--bg-app",
  settingsBg: "--bg-settings",
  accent: "--accent",
  textPrimary: "--text-primary",
  textMuted: "--text-muted",
  border: "--border-color",
  cardBg: "--card-bg",
  surfaceBg: "--surface-bg",
  statusBarBg: "--statusbar-bg",
  hoverBg: "--hover-bg",
  inputBg: "--input-bg",
  textSecondary: "--text-secondary",
  textFaint: "--text-faint",
  placeholderColor: "--placeholder-color",
  borderSubtle: "--border-subtle",
  selectionColor: "--selection-color",
  successColor: "--success-color",
  dangerColor: "--danger-color",
  warningColor: "--warning-color",
  scrollbarColor: "--scrollbar-color",
};

function cacheThemeColors(colors: ThemeColors, mode: ThemeMode): void {
  if (typeof window === "undefined") return;
  try {
    const vars: Record<string, string> = {};
    for (const key of Object.keys(COLOR_CSS_MAP) as (keyof ThemeColors)[]) {
      vars[COLOR_CSS_MAP[key]] = colors[key];
    }
    window.localStorage.setItem(THEME_CACHE_KEY, JSON.stringify(vars));
    window.localStorage.setItem(THEME_MODE_CACHE_KEY, mode);
  } catch {
    // Blocked or full storage only costs the next window its themed first paint.
  }
}

/**
 * System mode ignores saved colors from a previous OS appearance. Explicit
 * palettes retain their colors; missing preference information means light.
 */
export function resolveThemeColors(colors?: ThemeColors, mode?: ThemeMode): ThemeColors {
  if (mode === "system") return systemPrefersDark() ? DARK_THEME_COLORS : LIGHT_THEME_COLORS;
  return colors ?? (mode === "dark" || mode === "custom" ? DARK_THEME_COLORS : LIGHT_THEME_COLORS);
}

export function applyThemeColors(colors?: ThemeColors, mode: ThemeMode = "custom") {
  const c = resolveThemeColors(colors, mode);
  const r = document.documentElement.style;
  for (const key of Object.keys(COLOR_CSS_MAP) as (keyof ThemeColors)[]) {
    r.setProperty(COLOR_CSS_MAP[key], c[key]);
  }
  document.documentElement.style.colorScheme =
    resolveCodeTheme("auto", resolveThemeMode(mode), c.bg) === "vs-dark" ? "dark" : "light";
  cacheThemeColors(c, mode);
}
