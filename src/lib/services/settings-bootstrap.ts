import { getCurrentWindow } from "@tauri-apps/api/window";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  applyThemeColors,
  resolveThemeColors,
  resolveThemeMode,
} from "#lib/utils/settings/theme.ts";
import { applyCustomCss } from "#lib/utils/settings/custom-css.ts";
import { applyCodeTheme } from "#lib/utils/content/code-theme.ts";
import { isTauriRuntime } from "#lib/services/runtime.ts";
import type {
  FontSizeSettings,
  GeneralSettings,
  UiFontFamily,
  WindowEffect,
  ThemeColors,
} from "#lib/types/clipboard.ts";

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
 *
 * The shell factor tracks the slider one-to-one: an extra reduction there (it
 * used to be `opacity * 0.985`) made 100% render 1.5% of the desktop through the
 * main surface, so the configured color was never actually reached.
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
  root.style.setProperty("--window-opacity-shell", pct(opacity));
}

export function applyGeneralSettingsToDocument(settings: GeneralSettings): void {
  if (typeof document === "undefined") return;

  applyFontSizesToDocument(settings.fontSizes);
  const colors = resolveThemeColors(settings.themeColors, settings.theme);
  applyThemeColors(colors, settings.theme);
  applyCodeTheme(settings.codeTheme, resolveThemeMode(settings.theme), colors.bg);
  applyNativeBackdrop(colors);
  applyWindowEffectToDocument(settings.windowEffect);
  applyWindowOpacityToDocument(settings.windowTransparency, settings.windowOpacityAffectsText);
  applyCustomCss(settings.customCss, settings.customCssEnabled);
}

let lastNativeBackdrop: string | undefined;

/**
 * Native backdrop colors are a creation-time concern for the main window.
 *
 * `lib.rs` builds that window opaque only when the *saved* backdrop is opaque,
 * because WebView2 drops its initial white surface only when the default
 * background is set together with the controller. Writing a native backdrop
 * from the page is therefore one-way: lowering `windowTransparency` afterwards
 * only changes the CSS shell, which then composites over the opaque WebView2
 * surface it just painted, so the slider looks dead until a restart recreates
 * the window. Keep the main window's native surfaces exactly as created and let
 * CSS own its translucency.
 *
 * The settings and float windows stay opaque for their whole lifetime, so they
 * can follow live theme changes here (the float panel is seeded by
 * `revealFloatPanel` before it is shown).
 */
function applyNativeBackdrop(colors: ThemeColors): void {
  if (!isTauriRuntime()) return;
  const win = getCurrentWindow();
  if (win.label === "main") return;
  const bg = win.label === "settings" ? colors.settingsBg : colors.bg;
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
