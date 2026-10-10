// Assembles one complete `GeneralSettings` from an untrusted record: every field
// goes through `validators.ts`, unknown keys are dropped, and the derived
// structures (saved searches, custom CSS, theme presets) are normalized too.

import { normalizeCustomCss } from "$lib/utils/settings/custom-css";
import { normalizeSavedSearches } from "$lib/utils/settings/saved-searches";
import type { GeneralSettings } from "$lib/types/clipboard";
import { DARK_THEME_COLORS } from "$lib/types/clipboard";

import { DEFAULT_GENERAL_SETTINGS } from "./defaults";
import {
  booleanValue,
  cloneDefaults,
  integerInRange,
  isRecord,
  normalizeCustomPresets,
  normalizeIconColors,
  normalizeThemeColors,
  validCardActionsDisplay,
  validCacheEviction,
  validDetailDisplayMode,
  validFloatPanelClickAction,
  validFloatPanelPosition,
  validFullscreenMode,
  validGroupDisplayMode,
  validLanguage,
  validLogLevel,
  validSearchIndexSyncMode,
  validSearchPlaceholder,
  validSearchSuggestionMode,
  validSortRules,
  validTheme,
  validUpdateSource,
  validWindowEffect,
  type UnknownRecord,
} from "./validators";

/**
 * Merge and validate a backend/legacy payload while retaining unknown fields.
 * The Rust config deliberately has flattened extension fields, so rebuilding
 * only the known keys here would silently discard future settings.
 */
export function normalizeGeneralSettings(
  input: unknown,
  base: GeneralSettings = cloneDefaults(),
): GeneralSettings {
  const source = isRecord(input) ? input : {};
  const baseRecord = base as unknown as UnknownRecord;
  const sourceFontSizes = isRecord(source.fontSizes) ? source.fontSizes : {};
  const baseFontSizes = isRecord(baseRecord.fontSizes) ? baseRecord.fontSizes : {};
  const sourceDisplay = isRecord(source.display) ? source.display : {};
  const baseDisplay = isRecord(baseRecord.display) ? baseRecord.display : {};

  const result = {
    ...baseRecord,
    ...source,
    fontSizes: {
      ...baseFontSizes,
      ...sourceFontSizes,
    },
    display: {
      ...baseDisplay,
      ...sourceDisplay,
    },
  } as unknown as GeneralSettings;

  const defaultSettings = DEFAULT_GENERAL_SETTINGS;
  const fallback = (key: keyof GeneralSettings) => {
    const baseValue = (base as unknown as UnknownRecord)[key];
    return baseValue === undefined ? defaultSettings[key] : baseValue;
  };
  const fallbackFont = (key: keyof GeneralSettings["fontSizes"]) => {
    const baseValue = (base.fontSizes as unknown as UnknownRecord)[key];
    return baseValue === undefined ? defaultSettings.fontSizes[key] : baseValue;
  };
  const fallbackDisplay = (key: keyof GeneralSettings["display"]) => {
    const baseValue = (base.display as unknown as UnknownRecord)[key];
    return baseValue === undefined ? defaultSettings.display[key] : baseValue;
  };

  result.language = validLanguage(source.language ?? fallback("language"), "system");
  result.fontSizes.base = integerInRange(
    sourceFontSizes.base ?? fallbackFont("base"),
    defaultSettings.fontSizes.base,
    11,
    20,
  );
  result.fontSizes.secondary = integerInRange(
    sourceFontSizes.secondary ?? fallbackFont("secondary"),
    defaultSettings.fontSizes.secondary,
    9,
    16,
  );
  result.fontSizes.tiny = integerInRange(
    sourceFontSizes.tiny ?? fallbackFont("tiny"),
    defaultSettings.fontSizes.tiny,
    8,
    13,
  );
  result.fontSizes.cardTitle = integerInRange(
    sourceFontSizes.cardTitle ?? fallbackFont("cardTitle"),
    defaultSettings.fontSizes.cardTitle,
    10,
    20,
  );
  result.fontSizes.cardPreview = integerInRange(
    sourceFontSizes.cardPreview ?? fallbackFont("cardPreview"),
    defaultSettings.fontSizes.cardPreview,
    8,
    16,
  );
  result.display.showSecondaryText = booleanValue(
    sourceDisplay.showSecondaryText ?? fallbackDisplay("showSecondaryText"),
    defaultSettings.display.showSecondaryText,
  );
  result.display.maxTextLines = integerInRange(
    sourceDisplay.maxTextLines ?? fallbackDisplay("maxTextLines"),
    defaultSettings.display.maxTextLines,
    1,
    12,
  );
  result.display.pageSize = integerInRange(
    sourceDisplay.pageSize ?? fallbackDisplay("pageSize"),
    defaultSettings.display.pageSize,
    50,
    500,
  );
  result.display.searchPageSize = integerInRange(
    sourceDisplay.searchPageSize ?? fallbackDisplay("searchPageSize"),
    defaultSettings.display.searchPageSize,
    50,
    500,
  );
  result.windowTransparency = integerInRange(
    source.windowTransparency ?? fallback("windowTransparency"),
    defaultSettings.windowTransparency,
    60,
    100,
  );
  result.windowEffect = validWindowEffect(
    source.windowEffect ?? fallback("windowEffect"),
    defaultSettings.windowEffect,
  );
  result.windowOpacityAffectsText = booleanValue(
    source.windowOpacityAffectsText ?? fallback("windowOpacityAffectsText"),
    defaultSettings.windowOpacityAffectsText,
  );
  result.cardPaddingTop = integerInRange(
    source.cardPaddingTop ?? fallback("cardPaddingTop"),
    defaultSettings.cardPaddingTop,
    0,
    20,
  );
  result.cardPaddingBottom = integerInRange(
    source.cardPaddingBottom ?? fallback("cardPaddingBottom"),
    defaultSettings.cardPaddingBottom,
    0,
    20,
  );
  result.cardGap = integerInRange(
    source.cardGap ?? fallback("cardGap"),
    defaultSettings.cardGap,
    0,
    20,
  );
  result.cardTextHeight = integerInRange(
    source.cardTextHeight ?? fallback("cardTextHeight"),
    defaultSettings.cardTextHeight,
    36,
    90,
  );
  result.cardTallTextHeight = integerInRange(
    source.cardTallTextHeight ?? fallback("cardTallTextHeight"),
    defaultSettings.cardTallTextHeight,
    42,
    100,
  );
  result.cardImageHeight = integerInRange(
    source.cardImageHeight ?? fallback("cardImageHeight"),
    defaultSettings.cardImageHeight,
    64,
    200,
  );
  result.searchHeight = integerInRange(
    source.searchHeight ?? fallback("searchHeight"),
    defaultSettings.searchHeight,
    28,
    56,
  );
  result.searchFontSize = integerInRange(
    source.searchFontSize ?? fallback("searchFontSize"),
    defaultSettings.searchFontSize,
    10,
    24,
  );
  result.cardBorderRadius = integerInRange(
    source.cardBorderRadius ?? fallback("cardBorderRadius"),
    defaultSettings.cardBorderRadius,
    0,
    20,
  );
  result.pinCopiedToTop = booleanValue(
    source.pinCopiedToTop ?? fallback("pinCopiedToTop"),
    defaultSettings.pinCopiedToTop,
  );
  result.useRecycleBin = booleanValue(
    source.useRecycleBin ?? fallback("useRecycleBin"),
    defaultSettings.useRecycleBin,
  );
  result.pasteCleaningEnabled = booleanValue(
    source.pasteCleaningEnabled ?? fallback("pasteCleaningEnabled"),
    defaultSettings.pasteCleaningEnabled,
  );
  result.doubleClickPaste = booleanValue(
    source.doubleClickPaste ?? fallback("doubleClickPaste"),
    defaultSettings.doubleClickPaste,
  );
  result.showToastNotifications = booleanValue(
    source.showToastNotifications ?? fallback("showToastNotifications"),
    defaultSettings.showToastNotifications,
  );
  result.rememberWindowPosition = booleanValue(
    source.rememberWindowPosition ?? fallback("rememberWindowPosition"),
    defaultSettings.rememberWindowPosition,
  );
  result.alwaysOnTop = booleanValue(
    source.alwaysOnTop ?? fallback("alwaysOnTop"),
    defaultSettings.alwaysOnTop,
  );
  result.useSystemTitleBar = booleanValue(
    source.useSystemTitleBar ?? fallback("useSystemTitleBar"),
    defaultSettings.useSystemTitleBar,
  );
  result.theme = validTheme(source.theme ?? fallback("theme"), "dark");
  result.customCssEnabled = booleanValue(
    source.customCssEnabled ?? fallback("customCssEnabled"),
    false,
  );
  result.customCss = normalizeCustomCss(source.customCss ?? fallback("customCss"));
  result.themeColors = normalizeThemeColors(source.themeColors ?? fallback("themeColors"), {
    ...DARK_THEME_COLORS,
  });
  result.customPresets = normalizeCustomPresets(source.customPresets ?? fallback("customPresets"));
  // Like every other optional field, a *missing* value falls back to the
  // current store state so a partial payload (older backend, partial
  // broadcast) cannot silently clear the active theme preset. An own
  // `activePresetId: undefined` property is different: it is how
  // `updateSetting`/`merge` express an explicit clear (spreading keeps the
  // key), and it must win over the fallback or the clear is a silent no-op.
  result.activePresetId = Object.hasOwn(source, "activePresetId")
    ? typeof source.activePresetId === "string"
      ? source.activePresetId
      : undefined
    : (fallback("activePresetId") as string | undefined);
  result.imageFullscreenMode = validFullscreenMode(
    source.imageFullscreenMode ?? fallback("imageFullscreenMode"),
    "overlay",
  );
  result.viewerBackdropOpacity = integerInRange(
    source.viewerBackdropOpacity ?? fallback("viewerBackdropOpacity"),
    defaultSettings.viewerBackdropOpacity,
    0,
    100,
  );
  result.searchSuggestionMode = validSearchSuggestionMode(
    source.searchSuggestionMode ?? fallback("searchSuggestionMode"),
    defaultSettings.searchSuggestionMode,
  );
  result.searchHistoryEnabled = booleanValue(
    source.searchHistoryEnabled ?? fallback("searchHistoryEnabled"),
    defaultSettings.searchHistoryEnabled,
  );
  result.searchPlaceholder = validSearchPlaceholder(
    source.searchPlaceholder ?? fallback("searchPlaceholder"),
    defaultSettings.searchPlaceholder,
  );
  result.cardActionsDisplay = validCardActionsDisplay(
    source.cardActionsDisplay ?? fallback("cardActionsDisplay"),
    defaultSettings.cardActionsDisplay,
  );
  result.quickCopyBadgeAlwaysVisible = booleanValue(
    source.quickCopyBadgeAlwaysVisible ?? fallback("quickCopyBadgeAlwaysVisible"),
    defaultSettings.quickCopyBadgeAlwaysVisible,
  );
  result.showSettingsCloseButton = booleanValue(
    source.showSettingsCloseButton ?? fallback("showSettingsCloseButton"),
    defaultSettings.showSettingsCloseButton,
  );
  result.detailDisplayMode = validDetailDisplayMode(
    source.detailDisplayMode ?? fallback("detailDisplayMode"),
    defaultSettings.detailDisplayMode,
  );
  result.groupDisplayMode = validGroupDisplayMode(
    source.groupDisplayMode ?? fallback("groupDisplayMode"),
    defaultSettings.groupDisplayMode,
  );
  result.floatPanelPosition = validFloatPanelPosition(
    source.floatPanelPosition ?? fallback("floatPanelPosition"),
    defaultSettings.floatPanelPosition,
  );
  result.floatPanelLeftClick = validFloatPanelClickAction(
    source.floatPanelLeftClick ?? fallback("floatPanelLeftClick"),
    defaultSettings.floatPanelLeftClick,
  );
  result.floatPanelRightClick = validFloatPanelClickAction(
    source.floatPanelRightClick ?? fallback("floatPanelRightClick"),
    defaultSettings.floatPanelRightClick,
  );
  result.floatPanelMiddleClick = validFloatPanelClickAction(
    source.floatPanelMiddleClick ?? fallback("floatPanelMiddleClick"),
    defaultSettings.floatPanelMiddleClick,
  );
  result.searchSortRules = validSortRules(
    source.searchSortRules ?? fallback("searchSortRules"),
    defaultSettings.searchSortRules,
  );
  result.savedSearches = normalizeSavedSearches(source.savedSearches ?? fallback("savedSearches"));
  result.pageSizeLimit = integerInRange(
    source.pageSizeLimit ?? fallback("pageSizeLimit"),
    defaultSettings.pageSizeLimit,
    500,
    6000,
  );
  result.searchPageSizeLimit = integerInRange(
    source.searchPageSizeLimit ?? fallback("searchPageSizeLimit"),
    defaultSettings.searchPageSizeLimit,
    50,
    1000,
  );
  result.display.searchPageSize = Math.min(
    result.display.searchPageSize,
    result.searchPageSizeLimit,
  );
  result.maxTextCaptureBytes = integerInRange(
    source.maxTextCaptureBytes ?? fallback("maxTextCaptureBytes"),
    defaultSettings.maxTextCaptureBytes,
    10000,
    10000000,
  );
  result.searchCacheSize = integerInRange(
    source.searchCacheSize ?? fallback("searchCacheSize"),
    defaultSettings.searchCacheSize,
    200,
    2000,
  );
  result.searchCacheEviction = validCacheEviction(
    source.searchCacheEviction ?? fallback("searchCacheEviction"),
    defaultSettings.searchCacheEviction,
  );
  result.searchIndexSyncMode = validSearchIndexSyncMode(
    source.searchIndexSyncMode ?? fallback("searchIndexSyncMode"),
    defaultSettings.searchIndexSyncMode,
  );
  result.updateSource = validUpdateSource(
    source.updateSource ?? fallback("updateSource"),
    defaultSettings.updateSource,
  );
  result.colorIcons = booleanValue(
    source.colorIcons ?? fallback("colorIcons"),
    defaultSettings.colorIcons,
  );
  result.iconColors = normalizeIconColors(
    source.iconColors ?? fallback("iconColors"),
    fallback("iconColors") ?? {},
  );
  result.loadTolerance = integerInRange(
    source.loadTolerance ?? fallback("loadTolerance"),
    defaultSettings.loadTolerance,
    50,
    500,
  );
  result.logLevel = validLogLevel(
    source.logLevel ?? fallback("logLevel"),
    defaultSettings.logLevel,
  );

  return result;
}
