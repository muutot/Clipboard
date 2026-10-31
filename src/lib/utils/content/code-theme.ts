/**
 * Code highlighting theme: concrete palettes live as CSS variable scopes in
 * app.css (`[data-code-theme="…"]`). This module validates the setting,
 * resolves "auto" against the app palette, and applies the resolved scope to
 * the document root so every code surface follows.
 */

export const CODE_THEME_IDS = [
  "auto",
  "vs-dark",
  "one-dark",
  "github-light",
  "solarized-light",
] as const;

export type CodeThemeId = (typeof CODE_THEME_IDS)[number];
export type ConcreteCodeThemeId = Exclude<CodeThemeId, "auto">;

export const DEFAULT_CODE_THEME: CodeThemeId = "auto";

export function validCodeTheme(value: unknown, fallback: CodeThemeId): CodeThemeId {
  return typeof value === "string" && (CODE_THEME_IDS as readonly string[]).includes(value)
    ? (value as CodeThemeId)
    : fallback;
}

/** Perceived luminance of a #rrggbb color (0-1); "auto" needs it for custom palettes. */
function isLightColor(hex: string | undefined): boolean {
  if (!hex || !/^#[0-9a-fA-F]{6}/.test(hex)) return false;
  const r = parseInt(hex.slice(1, 3), 16) / 255;
  const g = parseInt(hex.slice(3, 5), 16) / 255;
  const b = parseInt(hex.slice(5, 7), 16) / 255;
  return 0.2126 * r + 0.7152 * g + 0.0722 * b > 0.55;
}

/** Resolves "auto" against the app theme; concrete ids pass through. */
export function resolveCodeTheme(
  codeTheme: string | undefined,
  appTheme: string,
  appBg: string | undefined,
): ConcreteCodeThemeId {
  if (
    codeTheme &&
    codeTheme !== "auto" &&
    (CODE_THEME_IDS as readonly string[]).includes(codeTheme)
  ) {
    return codeTheme as ConcreteCodeThemeId;
  }
  const light = appTheme === "light" || (appTheme === "custom" && isLightColor(appBg));
  return light ? "github-light" : "vs-dark";
}

export function applyCodeTheme(
  codeTheme: string | undefined,
  appTheme: string,
  appBg: string | undefined,
): void {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.codeTheme = resolveCodeTheme(codeTheme, appTheme, appBg);
}
