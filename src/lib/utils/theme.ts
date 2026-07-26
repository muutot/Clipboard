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
}
