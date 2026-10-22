import type { ThemeColors } from "$lib/types/clipboard";
import { DARK_THEME_COLORS } from "$lib/types/clipboard";

/**
 * Pre-paint theme snapshot. `static/theme-init.js` reads this key from `<head>`
 * before the first paint, so a light theme never flashes the dark defaults
 * declared in `app.html`/`app.css`. Keep the key and the variable mapping in
 * sync with that script; the snapshot is only a first-paint hint, never the
 * source of truth.
 */
export const THEME_CACHE_KEY = "clipboard-theme-vars";

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

function cacheThemeColors(colors: ThemeColors): void {
  if (typeof window === "undefined") return;
  try {
    const vars: Record<string, string> = {};
    for (const key of Object.keys(COLOR_CSS_MAP) as (keyof ThemeColors)[]) {
      vars[COLOR_CSS_MAP[key]] = colors[key];
    }
    window.localStorage.setItem(THEME_CACHE_KEY, JSON.stringify(vars));
  } catch {
    // Blocked or full storage only costs the next window its themed first paint.
  }
}

/**
 * Colors to paint when the store has none yet: the same dark defaults the CSS
 * declares, so document variables, the first-paint snapshot, and the native
 * window backdrop never disagree.
 */
export function resolveThemeColors(colors?: ThemeColors): ThemeColors {
  return colors ?? DARK_THEME_COLORS;
}

export function applyThemeColors(colors?: ThemeColors) {
  const c = resolveThemeColors(colors);
  const r = document.documentElement.style;
  for (const key of Object.keys(COLOR_CSS_MAP) as (keyof ThemeColors)[]) {
    r.setProperty(COLOR_CSS_MAP[key], c[key]);
  }
  cacheThemeColors(c);
}
