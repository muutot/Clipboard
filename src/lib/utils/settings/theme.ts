import type { ThemeColors } from "$lib/types/clipboard";
import { DARK_THEME_COLORS } from "$lib/types/clipboard";

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

export function applyThemeColors(colors?: ThemeColors) {
  const c = colors ?? DARK_THEME_COLORS;
  const r = document.documentElement.style;
  for (const key of Object.keys(COLOR_CSS_MAP) as (keyof ThemeColors)[]) {
    r.setProperty(COLOR_CSS_MAP[key], c[key]);
  }
}
