import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { applyThemeColors, resolveThemeColors, resolveThemeMode } from "$lib/utils/settings/theme";
import { applyCustomCss } from "$lib/utils/settings/custom-css";
import { applyCodeTheme } from "$lib/utils/content/code-theme";
import { isTauriRuntime } from "$lib/services/runtime";
import type {
  FontSizeSettings,
  GeneralSettings,
  UiFontFamily,
  WindowEffect,
  ThemeColors,
} from "$lib/types/clipboard";

/** Interface font stacks keyed by the `uiFontFamily` setting; the app.css
    fallback mirrors the mono stack so first paint needs no script. */
const UI_FONT_STACKS: Record<UiFontFamily, string> = {
  mono: `"Cascadia Code", "SFMono-Regular", Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", "Microsoft YaHei", sans-serif`,
  system: `"Segoe UI Variable Text", "Segoe UI", Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Microsoft YaHei", sans-serif`,
};

export function applyUiFontToDocument(fontFamily: UiFontFamily): void {
  if (typeof document === "undefined") return;
  document.documentElement.style.setProperty("--font-ui-family", UI_FONT_STACKS[fontFamily]);
}

export function applyFontSizesToDocument(fontSizes: FontSizeSettings): void {
  if (typeof document === "undefined") return;

  const root = document.documentElement;
  root.style.fontSize = `${fontSizes.base}px`;
  root.style.setProperty("--font-size-base", `${fontSizes.base}px`);
  root.style.setProperty("--font-size-secondary", `${fontSizes.secondary}px`);
  root.style.setProperty("--font-size-tiny", `${fontSizes.tiny}px`);
  root.style.setProperty("--font-size-cardTitle", `${fontSizes.cardTitle}px`);
  root.style.setProperty("--font-size-cardPreview", `${fontSizes.cardPreview}px`);
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

  applyFontSizesToDocument(settings.fontSizes);
  const colors = resolveThemeColors(settings.themeColors, settings.theme);
  applyThemeColors(colors, settings.theme);
  applyCodeTheme(settings.codeTheme, resolveThemeMode(settings.theme), colors.bg);
  applyNativeBackdrop(settings, colors);
  applyWindowEffectToDocument(settings.windowEffect);
  applyWindowOpacityToDocument(settings.windowTransparency, settings.windowOpacityAffectsText);
  applyCustomCss(settings.customCss, settings.customCssEnabled);
}

let lastNativeBackdrop: string | undefined;

/** Transparent main windows must retain their transparent native surfaces. */
function applyNativeBackdrop(settings: GeneralSettings, colors: ThemeColors): void {
  if (!isTauriRuntime()) return;
  const win = getCurrentWindow();
  const bg = win.label === "settings" ? colors.settingsBg : colors.bg;
  if (
    win.label === "main" &&
    (settings.windowEffect !== "off" ||
      settings.windowOpacityAffectsText ||
      settings.windowTransparency < 100)
  )
    return;
  const key = `${win.label}:${bg}`;
  if (lastNativeBackdrop === key) return;
  lastNativeBackdrop = key;
  void Promise.all([win.setBackgroundColor(bg), getCurrentWebview().setBackgroundColor(bg)]).catch(
    (error) => {
      if (lastNativeBackdrop === key) lastNativeBackdrop = undefined;
      console.error("Unable to apply the native theme backdrop", error);
    },
  );
}
