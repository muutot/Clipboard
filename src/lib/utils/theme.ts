import type { ThemeColors } from "$lib/types/clipboard";
import { DARK_THEME_COLORS } from "$lib/types/clipboard";

export function applyThemeColors(colors?: ThemeColors) {
  const c = colors ?? DARK_THEME_COLORS;
  const r = document.documentElement.style;
  r.setProperty("--bg-app", c.bg);
  r.setProperty("--bg-settings", c.settingsBg);
  r.setProperty("--accent", c.accent);
  r.setProperty("--text-primary", c.textPrimary);
  r.setProperty("--text-muted", c.textMuted);
  r.setProperty("--border-color", c.border);
  r.setProperty("--card-bg", c.cardBg);
  r.setProperty("--surface-bg", c.surfaceBg);
  r.setProperty("--statusbar-bg", c.statusBarBg);
  r.setProperty("--hover-bg", c.hoverBg);
  r.setProperty("--input-bg", c.inputBg);
  r.setProperty("--text-secondary", c.textSecondary);
  r.setProperty("--text-faint", c.textFaint);
  r.setProperty("--placeholder-color", c.placeholderColor);
  r.setProperty("--border-subtle", c.borderSubtle);
  r.setProperty("--selection-color", c.selectionColor);
  r.setProperty("--success-color", c.successColor);
  r.setProperty("--danger-color", c.dangerColor);
  r.setProperty("--warning-color", c.warningColor);
  r.setProperty("--scrollbar-color", c.scrollbarColor);
}
