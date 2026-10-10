// Field-level validation and normalization for settings arriving from disk, IPC,
// or localStorage: each helper coerces an `unknown` into one typed field.
// `normalize.ts` assembles a whole record out of these.

import type {
  FloatPanelClickAction,
  GeneralSettings,
  Language,
  SortRule,
  ThemeColors,
  ThemePreset,
} from "$lib/types/clipboard";
import { DARK_THEME_COLORS, ICON_NAMES, type IconColors } from "$lib/types/clipboard";

import { DEFAULT_GENERAL_SETTINGS } from "./defaults";

export type UnknownRecord = Record<string, unknown>;

export function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function cloneDefaults(): GeneralSettings {
  const defaults = DEFAULT_GENERAL_SETTINGS;
  return {
    ...defaults,
    fontSizes: { ...defaults.fontSizes },
    display: { ...defaults.display },
    themeColors: defaults.themeColors ? { ...defaults.themeColors } : undefined,
    iconColors: defaults.iconColors ? { ...defaults.iconColors } : {},
    customPresets: defaults.customPresets.map((preset) => ({
      ...preset,
      colors: { ...preset.colors },
    })),
    searchSortRules: defaults.searchSortRules.map((rule) => ({ ...rule })),
    savedSearches: [],
  };
}

export function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

export function integerInRange(value: unknown, fallback: number, min: number, max: number): number {
  return Math.round(Math.min(max, Math.max(min, finiteNumber(value, fallback))));
}

export function booleanValue(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

export function validLanguage(value: unknown, fallback: Language): Language {
  return value === "system" || value === "zh-CN" || value === "en" ? value : fallback;
}

export function validTheme(
  value: unknown,
  fallback: GeneralSettings["theme"],
): GeneralSettings["theme"] {
  return value === "dark" || value === "light" || value === "custom" ? value : fallback;
}

export function validHexColor(value: unknown, fallback: string): string {
  return typeof value === "string" && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(value)
    ? value
    : fallback;
}

const THEME_COLOR_KEYS = Object.keys(DARK_THEME_COLORS) as (keyof ThemeColors)[];

export function normalizeThemeColors(source: unknown, fallback: ThemeColors): ThemeColors {
  const result: Record<string, string> = { ...fallback };
  if (!source || typeof source !== "object") return result as unknown as ThemeColors;
  const src = source as Record<string, unknown>;
  for (const key of THEME_COLOR_KEYS) {
    result[key] = validHexColor(src[key], (fallback as unknown as Record<string, string>)[key]);
  }
  return result as unknown as ThemeColors;
}

export function normalizeIconColors(source: unknown, fallback: IconColors): IconColors {
  if (!source || typeof source !== "object" || Array.isArray(source)) return { ...fallback };
  const src = source as Record<string, unknown>;
  const result: IconColors = {};
  for (const name of ICON_NAMES) {
    const cleaned = validHexColor(src[name], "");
    if (cleaned) result[name] = cleaned;
  }
  return result;
}

export function normalizeCustomPresets(source: unknown): ThemePreset[] {
  if (!Array.isArray(source)) return [];
  return source
    .filter(
      (item): item is Record<string, unknown> =>
        isRecord(item) && typeof item.id === "string" && typeof item.name === "string",
    )
    .map((item) => ({
      id: item.id as string,
      name: item.name as string,
      colors: normalizeThemeColors(item.colors, { ...DARK_THEME_COLORS }),
    }));
}

export function validSearchSuggestionMode(
  value: unknown,
  fallback: GeneralSettings["searchSuggestionMode"],
): GeneralSettings["searchSuggestionMode"] {
  return value === "off" || value === "panel" || value === "inline" ? value : fallback;
}

export function validCardActionsDisplay(
  value: unknown,
  fallback: GeneralSettings["cardActionsDisplay"],
): GeneralSettings["cardActionsDisplay"] {
  return value === "hover" || value === "always" ? value : fallback;
}

export function validWindowEffect(
  value: unknown,
  fallback: GeneralSettings["windowEffect"],
): GeneralSettings["windowEffect"] {
  return value === "off" || value === "acrylic" || value === "mica" ? value : fallback;
}

export function validFullscreenMode(
  value: unknown,
  fallback: GeneralSettings["imageFullscreenMode"],
): GeneralSettings["imageFullscreenMode"] {
  return value === "overlay" || value === "desktop" ? value : fallback;
}

export function validDetailDisplayMode(
  value: unknown,
  fallback: GeneralSettings["detailDisplayMode"],
): GeneralSettings["detailDisplayMode"] {
  return value === "overlay" || value === "split" ? value : fallback;
}

export function validGroupDisplayMode(
  value: unknown,
  fallback: GeneralSettings["groupDisplayMode"],
): GeneralSettings["groupDisplayMode"] {
  return value === "iconText" || value === "iconOnly" || value === "textOnly" ? value : fallback;
}

export function validFloatPanelClickAction(
  value: unknown,
  fallback: FloatPanelClickAction,
): FloatPanelClickAction {
  return value === "none" ||
    value === "copy" ||
    value === "copyPaste" ||
    value === "favorite" ||
    value === "detail" ||
    value === "delete"
    ? value
    : fallback;
}

export function validFloatPanelPosition(
  value: unknown,
  fallback: GeneralSettings["floatPanelPosition"],
): GeneralSettings["floatPanelPosition"] {
  return value === "topLeft" ||
    value === "topRight" ||
    value === "bottomLeft" ||
    value === "bottomRight" ||
    value === "center"
    ? value
    : fallback;
}

const SORT_FIELDS = ["createdAt", "lastUsedAt", "title", "size", "kind", "favorite"] as const;

export function validSortRules(value: unknown, fallback: SortRule[]): SortRule[] {
  if (!Array.isArray(value)) return fallback;
  const rules: SortRule[] = [];
  for (const item of value) {
    if (!isRecord(item)) continue;
    if (!SORT_FIELDS.includes(item.field as (typeof SORT_FIELDS)[number])) continue;
    if (item.direction !== "asc" && item.direction !== "desc") continue;
    rules.push({ field: item.field as SortRule["field"], direction: item.direction });
  }
  return rules.length > 0 ? rules : fallback;
}

export function validCacheEviction(value: unknown, fallback: "fifo" | "lru"): "fifo" | "lru" {
  return value === "fifo" || value === "lru" ? value : fallback;
}

export function validSearchIndexSyncMode(
  value: unknown,
  fallback: "lazy" | "background",
): "lazy" | "background" {
  return value === "lazy" || value === "background" ? value : fallback;
}

export function validUpdateSource(
  value: unknown,
  fallback: "github" | "gitcode",
): "github" | "gitcode" {
  return value === "github" || value === "gitcode" ? value : fallback;
}

export function validLogLevel(
  value: unknown,
  fallback: GeneralSettings["logLevel"],
): GeneralSettings["logLevel"] {
  return value === "error" || value === "warn" || value === "info" || value === "debug"
    ? value
    : fallback;
}

export function validSearchPlaceholder(value: unknown, fallback: string): string {
  if (typeof value !== "string") return fallback;
  return value.trim().slice(0, 80);
}
