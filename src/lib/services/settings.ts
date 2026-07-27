import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { get, writable } from "svelte/store";
import { setLocale } from "$lib/i18n";
import { isTauriRuntime } from "$lib/services/runtime";
import type {
  GeneralSettings,
  GeneralSettingsInfo,
  Language,
  SortRule,
  ThemeColors,
  ThemePreset,
  WindowConfig,
  WindowPosition,
} from "$lib/types/clipboard";
import { DARK_THEME_COLORS, LIGHT_THEME_COLORS } from "$lib/types/clipboard";

const STORAGE_KEY = "generalSettings";
const LOCALE_STORAGE_KEY = "clipboard-locale";
const PERSIST_DEBOUNCE_MS = 120;

export const DEFAULT_GENERAL_SETTINGS: GeneralSettings = {
  language: "zh-CN",
  fontSizes: { base: 14, secondary: 11, tiny: 10, cardTitle: 13, cardPreview: 11 },
  display: { showSecondaryText: true, maxTextLines: 3, pageSize: 100 },
  windowTransparency: 95,
  compactMode: false,
  compactPaddingTop: 6,
  compactPaddingBottom: 4,
  compactCardGap: 5,
  compactTextHeight: 58,
  compactTallTextHeight: 70,
  compactImageHeight: 130,
  compactCustomTitleHeight: 80,
  compactSearchHeight: 40,
  compactSearchFontSize: 14,
  compactCardBorderRadius: 10,
  pinCopiedToTop: true,
  useRecycleBin: true,
  showToastNotifications: true,
  rememberWindowPosition: false,
  alwaysOnTop: false,
  useSystemTitleBar: false,
  theme: "dark",
  themeColors: { ...DARK_THEME_COLORS },
  customPresets: [],
  activePresetId: undefined,
  imageFullscreenMode: "overlay",
  viewerBackdropOpacity: 92,
  searchSuggestionMode: "off",
  searchHistoryEnabled: false,
  cardActionsDisplay: "hover",
  quickCopyBadgeAlwaysVisible: true,
  showSettingsCloseButton: true,
  detailDisplayMode: "overlay",
  searchSortRules: [{ field: "createdAt", direction: "desc" }],
  pageSizeLimit: 500,
  searchPageSizeLimit: 500,
};

type UnknownRecord = Record<string, unknown>;

function isRecord(value: unknown): value is UnknownRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function cloneDefaults(): GeneralSettings {
  return {
    ...DEFAULT_GENERAL_SETTINGS,
    fontSizes: { ...DEFAULT_GENERAL_SETTINGS.fontSizes },
    display: { ...DEFAULT_GENERAL_SETTINGS.display },
  };
}

function finiteNumber(value: unknown, fallback: number): number {
  return typeof value === "number" && Number.isFinite(value) ? value : fallback;
}

function integerInRange(value: unknown, fallback: number, min: number, max: number): number {
  return Math.round(Math.min(max, Math.max(min, finiteNumber(value, fallback))));
}

function booleanValue(value: unknown, fallback: boolean): boolean {
  return typeof value === "boolean" ? value : fallback;
}

function validLanguage(value: unknown, fallback: Language): Language {
  return value === "zh-CN" || value === "en" ? value : fallback;
}

function validTheme(value: unknown, fallback: GeneralSettings["theme"]): GeneralSettings["theme"] {
  return value === "dark" || value === "light" || value === "custom" ? value : fallback;
}

function validHexColor(value: unknown, fallback: string): string {
  return typeof value === "string" && /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/.test(value)
    ? value
    : fallback;
}

const THEME_COLOR_KEYS: (keyof ThemeColors)[] = [
  "bg",
  "settingsBg",
  "accent",
  "textPrimary",
  "textMuted",
  "border",
  "cardBg",
  "surfaceBg",
  "statusBarBg",
  "hoverBg",
  "inputBg",
  "textSecondary",
  "textFaint",
  "placeholderColor",
  "borderSubtle",
  "selectionColor",
  "successColor",
  "dangerColor",
  "warningColor",
  "scrollbarColor",
];

function normalizeThemeColors(source: unknown, fallback: ThemeColors): ThemeColors {
  const result: Record<string, string> = { ...fallback };
  if (!source || typeof source !== "object") return result as unknown as ThemeColors;
  const src = source as Record<string, unknown>;
  for (const key of THEME_COLOR_KEYS) {
    result[key] = validHexColor(src[key], (fallback as unknown as Record<string, string>)[key]);
  }
  return result as unknown as ThemeColors;
}

function normalizeCustomPresets(source: unknown): ThemePreset[] {
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

function validSearchSuggestionMode(
  value: unknown,
  fallback: GeneralSettings["searchSuggestionMode"],
): GeneralSettings["searchSuggestionMode"] {
  return value === "off" || value === "panel" || value === "inline" ? value : fallback;
}

function validCardActionsDisplay(
  value: unknown,
  fallback: GeneralSettings["cardActionsDisplay"],
): GeneralSettings["cardActionsDisplay"] {
  return value === "hover" || value === "always" ? value : fallback;
}

function validFullscreenMode(
  value: unknown,
  fallback: GeneralSettings["imageFullscreenMode"],
): GeneralSettings["imageFullscreenMode"] {
  return value === "overlay" || value === "desktop" ? value : fallback;
}

function validDetailDisplayMode(
  value: unknown,
  fallback: GeneralSettings["detailDisplayMode"],
): GeneralSettings["detailDisplayMode"] {
  return value === "overlay" || value === "split" ? value : fallback;
}

const SORT_FIELDS = ["createdAt", "lastUsedAt", "title", "size", "kind", "favorite"] as const;

function validSortRules(value: unknown, fallback: SortRule[]): SortRule[] {
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

/**
 * Merge and validate a backend/legacy payload while retaining unknown fields.
 * The Rust config deliberately has flattened extension fields, so rebuilding
 * only the known keys here would silently discard future settings.
 */
function normalizeGeneralSettings(
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

  result.language = validLanguage(source.language ?? fallback("language"), "zh-CN");
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
  result.windowTransparency = integerInRange(
    source.windowTransparency ?? fallback("windowTransparency"),
    defaultSettings.windowTransparency,
    60,
    100,
  );
  result.compactMode = booleanValue(
    source.compactMode ?? fallback("compactMode"),
    defaultSettings.compactMode,
  );
  result.compactPaddingTop = integerInRange(
    source.compactPaddingTop ?? fallback("compactPaddingTop"),
    defaultSettings.compactPaddingTop,
    0,
    20,
  );
  result.compactPaddingBottom = integerInRange(
    source.compactPaddingBottom ?? fallback("compactPaddingBottom"),
    defaultSettings.compactPaddingBottom,
    0,
    20,
  );
  result.compactCardGap = integerInRange(
    source.compactCardGap ?? fallback("compactCardGap"),
    defaultSettings.compactCardGap,
    0,
    20,
  );
  result.compactTextHeight = integerInRange(
    source.compactTextHeight ?? fallback("compactTextHeight"),
    defaultSettings.compactTextHeight,
    36,
    90,
  );
  result.compactTallTextHeight = integerInRange(
    source.compactTallTextHeight ?? fallback("compactTallTextHeight"),
    defaultSettings.compactTallTextHeight,
    44,
    100,
  );
  result.compactImageHeight = integerInRange(
    source.compactImageHeight ?? fallback("compactImageHeight"),
    defaultSettings.compactImageHeight,
    64,
    200,
  );
  result.compactCustomTitleHeight = integerInRange(
    source.compactCustomTitleHeight ?? fallback("compactCustomTitleHeight"),
    defaultSettings.compactCustomTitleHeight,
    40,
    120,
  );
  result.compactSearchHeight = integerInRange(
    source.compactSearchHeight ?? fallback("compactSearchHeight"),
    defaultSettings.compactSearchHeight,
    28,
    56,
  );
  result.compactSearchFontSize = integerInRange(
    source.compactSearchFontSize ?? fallback("compactSearchFontSize"),
    defaultSettings.compactSearchFontSize,
    10,
    24,
  );
  result.compactCardBorderRadius = integerInRange(
    source.compactCardBorderRadius ?? fallback("compactCardBorderRadius"),
    defaultSettings.compactCardBorderRadius,
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
  result.themeColors = normalizeThemeColors(source.themeColors ?? fallback("themeColors"), {
    ...DARK_THEME_COLORS,
  });
  result.customPresets = normalizeCustomPresets(source.customPresets ?? fallback("customPresets"));
  result.activePresetId =
    typeof source.activePresetId === "string" ? source.activePresetId : undefined;
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
  result.searchSortRules = validSortRules(
    source.searchSortRules ?? fallback("searchSortRules"),
    defaultSettings.searchSortRules,
  );
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

  return result;
}

function readStorage(key: string): string | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function removeStorage(key: string): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.removeItem(key);
  } catch {
    // localStorage may be unavailable in a restricted webview.
  }
}

function parseStorageObject(key: string): unknown {
  const raw = readStorage(key);
  if (!raw) return undefined;
  try {
    return JSON.parse(raw);
  } catch {
    return undefined;
  }
}

function readLegacySettings(): { settings: unknown; locale?: Language } {
  const generalRaw = readStorage(STORAGE_KEY);
  const localeRaw = readStorage(LOCALE_STORAGE_KEY);
  const locale = localeRaw === "zh-CN" || localeRaw === "en" ? localeRaw : undefined;
  return {
    settings: parseStorageObject(STORAGE_KEY),
    locale,
  };
}

function saveBrowserSettings(value: GeneralSettings): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
  } catch {
    // localStorage may be unavailable in a restricted browser context.
  }
}

export async function getGeneralSettings(): Promise<GeneralSettingsInfo> {
  if (!isTauriRuntime()) {
    return {
      settings: cloneDefaults(),
      legacyMigrationRequired: false,
    };
  }
  return invoke<GeneralSettingsInfo>("get_general_settings");
}

export async function setGeneralSettings(value: GeneralSettings): Promise<GeneralSettings> {
  const settings = normalizeGeneralSettings(value);
  if (!isTauriRuntime()) return settings;
  return invoke<GeneralSettings>("set_general_settings", { settings });
}

export async function getWindowConfig(): Promise<WindowConfig> {
  if (!isTauriRuntime()) {
    return {
      launchAtStartup: false,
      closeToTray: true,
      singleInstance: true,
    };
  }
  return invoke<WindowConfig>("get_window_config");
}

export async function setWindowConfig(settings: Partial<WindowConfig>): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("set_window_config", {
    launchAtStartup: settings.launchAtStartup ?? null,
    closeToTray: settings.closeToTray ?? null,
    singleInstance: settings.singleInstance ?? null,
  });
}

export async function restoreWindowPosition(): Promise<WindowPosition | null> {
  if (!isTauriRuntime()) return null;
  return invoke<WindowPosition | null>("restore_window_position");
}

export async function saveWindowPosition(position: WindowPosition): Promise<void> {
  if (!isTauriRuntime()) return;
  await invoke("save_window_position", { ...position });
}

function applyDirtySettings(
  remote: GeneralSettings,
  local: GeneralSettings,
  dirtyKeys: Set<keyof GeneralSettings>,
): GeneralSettings {
  if (dirtyKeys.size === 0) return remote;
  const merged = { ...remote } as GeneralSettings;
  for (const key of dirtyKeys) {
    (merged as unknown as UnknownRecord)[key] = (local as unknown as UnknownRecord)[key];
  }
  return normalizeGeneralSettings(merged, remote);
}

function createSettingsStore() {
  const desktop = isTauriRuntime();
  const browserInitial = normalizeGeneralSettings(parseStorageObject(STORAGE_KEY), cloneDefaults());
  const store = writable<GeneralSettings>(desktop ? cloneDefaults() : browserInitial);
  let applyingExternalValue = false;
  let initialized = !desktop;
  let initialization: Promise<void> | undefined;
  let localRevision = 0;
  const dirtyKeys = new Set<keyof GeneralSettings>();
  let pendingValue: GeneralSettings | undefined;
  let writeTimer: ReturnType<typeof setTimeout> | undefined;
  let writeInFlight: Promise<void> | undefined;
  let unlistenSettings: (() => void) | undefined;
  let legacyMigrationPending = false;

  if (!desktop && typeof window !== "undefined") {
    store.subscribe((value) => {
      if (!applyingExternalValue) saveBrowserSettings(value);
    });
    window.addEventListener("storage", (event) => {
      if (event.key !== STORAGE_KEY || !event.newValue) return;
      try {
        applyingExternalValue = true;
        store.set(normalizeGeneralSettings(JSON.parse(event.newValue), get(store)));
      } catch {
        // Ignore malformed values from another browser tab.
      } finally {
        applyingExternalValue = false;
      }
    });
  }

  function schedulePersist(): void {
    if (!desktop || !initialized) return;
    pendingValue = normalizeGeneralSettings(get(store), get(store));
    if (writeTimer !== undefined) clearTimeout(writeTimer);
    writeTimer = setTimeout(() => {
      writeTimer = undefined;
      void drainWrites().catch(() => {});
    }, PERSIST_DEBOUNCE_MS);
  }

  function drainWrites(): Promise<void> {
    if (!desktop) return Promise.resolve();
    if (writeInFlight) return writeInFlight;

    writeInFlight = (async () => {
      while (pendingValue) {
        const value = pendingValue;
        pendingValue = undefined;
        try {
          const saved = await setGeneralSettings(value);
          // A newer value may have arrived while this write was in flight.
          // In that case the next loop iteration owns the store update.
          if (!pendingValue) {
            const normalized = normalizeGeneralSettings(saved, get(store));
            store.set(normalized);
            setLocale(normalized.language);
            if (legacyMigrationPending) {
              removeStorage(STORAGE_KEY);
              removeStorage(LOCALE_STORAGE_KEY);
              legacyMigrationPending = false;
            }
          }
        } catch (error) {
          // Keep the latest failed value so an explicit flush or a later edit
          // can retry it; do not spin on a permanently failed IPC call.
          pendingValue = value;
          throw error;
        }
      }
    })().finally(() => {
      writeInFlight = undefined;
    });
    return writeInFlight;
  }

  async function initialize(): Promise<void> {
    if (initialization) return initialization;
    initialization = (async () => {
      if (!desktop) {
        initialized = true;
        return;
      }

      try {
        unlistenSettings = await listen<GeneralSettings>("general-settings-changed", (event) => {
          // Ignore an event while our own command is in flight; its command
          // response is the canonical value we apply below.
          if (writeInFlight || pendingValue) return;
          const normalized = normalizeGeneralSettings(event.payload, get(store));
          store.set(normalized);
          setLocale(normalized.language);
        });
      } catch {
        // A missing event permission must not prevent settings persistence.
      }

      let response: GeneralSettingsInfo;
      try {
        response = await getGeneralSettings();
      } catch {
        initialized = true;
        if (dirtyKeys.size > 0) schedulePersist();
        return;
      }

      const localAtHydration = get(store);
      const dirtyAtHydration = new Set(dirtyKeys);
      const revisionAtHydration = localRevision;
      let hydrated = normalizeGeneralSettings(response.settings, cloneDefaults());

      if (response.legacyMigrationRequired) {
        legacyMigrationPending = true;
        const legacy = readLegacySettings();
        hydrated = normalizeGeneralSettings(legacy.settings, hydrated);
        const legacyRecord = isRecord(legacy.settings) ? legacy.settings : {};
        if (legacy.locale && legacyRecord.language !== "zh-CN" && legacyRecord.language !== "en") {
          hydrated.language = legacy.locale;
        }
        hydrated = applyDirtySettings(hydrated, localAtHydration, dirtyAtHydration);
        try {
          hydrated = normalizeGeneralSettings(await setGeneralSettings(hydrated), hydrated);
          const concurrentEdits = localRevision !== revisionAtHydration;
          if (concurrentEdits) {
            hydrated = applyDirtySettings(hydrated, get(store), dirtyKeys);
            pendingValue = hydrated;
          }
          if (!concurrentEdits) {
            removeStorage(STORAGE_KEY);
            removeStorage(LOCALE_STORAGE_KEY);
            legacyMigrationPending = false;
            dirtyKeys.clear();
          }
        } catch {
          // Keep old keys for a retry if the first migration write fails.
          pendingValue = applyDirtySettings(hydrated, get(store), dirtyKeys);
        }
      } else {
        removeStorage(STORAGE_KEY);
        removeStorage(LOCALE_STORAGE_KEY);
        hydrated = applyDirtySettings(hydrated, localAtHydration, dirtyAtHydration);
      }

      store.set(hydrated);
      setLocale(hydrated.language);
      initialized = true;
      if (pendingValue || (dirtyAtHydration.size > 0 && !response.legacyMigrationRequired)) {
        schedulePersist();
      }
    })();
    return initialization;
  }

  function updateSetting<K extends keyof GeneralSettings>(key: K, value: GeneralSettings[K]) {
    localRevision += 1;
    dirtyKeys.add(key);
    const current = get(store);
    const next = normalizeGeneralSettings({ ...current, [key]: value }, current);
    store.set(next);
    schedulePersist();
  }

  function merge(partial: Partial<GeneralSettings>) {
    localRevision += 1;
    for (const key of Object.keys(partial) as Array<keyof GeneralSettings>) {
      dirtyKeys.add(key);
    }
    const current = get(store);
    const next = normalizeGeneralSettings({ ...current, ...partial }, current);
    store.set(next);
    schedulePersist();
  }

  async function flush(): Promise<void> {
    if (!desktop) return;
    if (!initialized && initialization) await initialization;
    if (writeTimer !== undefined) {
      clearTimeout(writeTimer);
      writeTimer = undefined;
    }
    await drainWrites();
  }

  void initialize();

  return {
    ...store,
    updateSetting,
    merge,
    initialize,
    flush,
    /** Exposed for lifecycle cleanup in tests and future window teardown. */
    destroy() {
      if (writeTimer !== undefined) clearTimeout(writeTimer);
      writeTimer = undefined;
      unlistenSettings?.();
      unlistenSettings = undefined;
    },
  };
}

export const generalSettings = createSettingsStore();
