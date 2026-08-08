import { getCurrentWindow } from "@tauri-apps/api/window";
import { applyThemeColors } from "$lib/utils/theme";
import { isTauriRuntime } from "$lib/services/runtime";
import type {
  FontSizeSettings,
  DisplaySettings,
  GeneralSettings,
  WindowEffect,
} from "$lib/types/clipboard";

export function applyFontSizesToDocument(
  fontSizes: FontSizeSettings,
  display?: Pick<DisplaySettings, "showSecondaryText">,
): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  root.style.fontSize = `${fontSizes.base}px`;
  root.style.setProperty("--font-size-base", `${fontSizes.base}px`);
  root.style.setProperty("--font-size-secondary", `${fontSizes.secondary}px`);
  root.style.setProperty("--font-size-tiny", `${fontSizes.tiny}px`);
  root.style.setProperty("--font-size-cardTitle", `${fontSizes.cardTitle}px`);
  root.style.setProperty("--font-size-cardPreview", `${fontSizes.cardPreview}px`);

  if (display) {
    root.style.setProperty("--show-secondary", display.showSecondaryText ? "block" : "none");
  }
}

export function applyWindowEffectToDocument(effect: WindowEffect): void {
  if (typeof document === "undefined") return;
  if (!isTauriRuntime()) return;
  if (getCurrentWindow().label !== "main") return;
  if (effect === "off") {
    delete document.documentElement.dataset.windowEffect;
  } else {
    document.documentElement.dataset.windowEffect = effect;
  }
}

/**
 * Applies the configured window transparency as concrete color-mix percentages
 * on the backdrop surfaces. When the window transparency also dims the text by
 * design (native layered alpha active), the CSS factors are kept at full
 * opacity so the translucency is not applied twice.
 */
export function applyWindowOpacityToDocument(transparency: number, affectsText: boolean): void {
  if (typeof document === "undefined") return;
  if (!isTauriRuntime()) return;
  if (getCurrentWindow().label !== "main") return;
  const root = document.documentElement;
  if (affectsText) {
    root.style.removeProperty("--window-opacity-mix");
    root.style.removeProperty("--window-opacity-glass");
    root.style.removeProperty("--window-opacity-shell");
    return;
  }
  const opacity = Math.max(0.6, Math.min(1, transparency / 100));
  const pct = (value: number) => `${(value * 100).toFixed(1)}%`;
  root.style.setProperty("--window-opacity-mix", pct(opacity));
  root.style.setProperty("--window-opacity-glass", pct(opacity * 0.42));
  root.style.setProperty("--window-opacity-shell", pct(opacity * 0.985));
}

export function applyGeneralSettingsToDocument(settings: GeneralSettings): void {
  if (typeof document === "undefined") return;

  applyFontSizesToDocument(settings.fontSizes, settings.display);
  applyThemeColors(settings.themeColors);
  applyWindowEffectToDocument(settings.windowEffect);
  applyWindowOpacityToDocument(settings.windowTransparency, settings.windowOpacityAffectsText);
}

export function syncCompactShellClass(compactMode: boolean): void {
  if (typeof document === "undefined") return;

  const shell = document.querySelector<HTMLElement>(".app-shell");
  if (shell) {
    shell.classList.toggle("compact", compactMode);
  }
}
