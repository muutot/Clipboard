import { applyThemeColors } from "$lib/utils/theme";
import type { GeneralSettings } from "$lib/types/clipboard";

export function applyGeneralSettingsToDocument(settings: GeneralSettings): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  const { fontSizes, display } = settings;

  root.style.fontSize = `${fontSizes.base}px`;
  root.style.setProperty("--font-size-base", `${fontSizes.base}px`);
  root.style.setProperty("--font-size-secondary", `${fontSizes.secondary}px`);
  root.style.setProperty("--font-size-tiny", `${fontSizes.tiny}px`);
  root.style.setProperty("--font-size-cardTitle", `${fontSizes.cardTitle}px`);
  root.style.setProperty("--font-size-cardPreview", `${fontSizes.cardPreview}px`);
  root.style.setProperty("--show-secondary", display.showSecondaryText ? "block" : "none");

  applyThemeColors(settings.themeColors);
}

export function syncCompactShellClass(compactMode: boolean): void {
  if (typeof document === "undefined") return;

  const shell = document.querySelector<HTMLElement>(".app-shell");
  if (shell) {
    shell.classList.toggle("compact", compactMode);
  }
}
