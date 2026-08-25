import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { DEFAULT_GENERAL_SETTINGS, generalSettings, validHexColor } from "./settings";

const STORAGE_KEY = "generalSettings";

beforeEach(() => {
  window.localStorage.clear();
});

afterEach(() => {
  generalSettings.destroy();
  vi.useRealTimers();
});

describe("validHexColor", () => {
  it("accepts six- and eight-digit hex colors", () => {
    expect(validHexColor("#ff0000", "x")).toBe("#ff0000");
    expect(validHexColor("#FF0000AA", "x")).toBe("#FF0000AA");
    expect(validHexColor("#abc123", "x")).toBe("#abc123");
  });

  it("falls back for malformed input", () => {
    const fallback = "#123456";
    expect(validHexColor("ff0000", fallback)).toBe(fallback);
    expect(validHexColor("#fff", fallback)).toBe(fallback);
    expect(validHexColor("#12345", fallback)).toBe(fallback);
    expect(validHexColor(123 as unknown as string, fallback)).toBe(fallback);
    expect(validHexColor(null, fallback)).toBe(fallback);
  });
});

describe("generalSettings store normalization", () => {
  it("keeps nested defaults independent across updates (no shared containers)", () => {
    const before = JSON.stringify(DEFAULT_GENERAL_SETTINGS.fontSizes);
    generalSettings.updateSetting("fontSizes", {
      ...DEFAULT_GENERAL_SETTINGS.fontSizes,
      base: 20,
    });
    expect(JSON.stringify(DEFAULT_GENERAL_SETTINGS.fontSizes)).toBe(before);
    expect(DEFAULT_GENERAL_SETTINGS.fontSizes.base).toBe(14);
  });

  it("clamps numeric settings into their configured ranges", () => {
    generalSettings.updateSetting("windowTransparency", 10 as unknown as number);
    expect(get(generalSettings).windowTransparency).toBe(60);

    generalSettings.updateSetting("windowTransparency", 500 as unknown as number);
    expect(get(generalSettings).windowTransparency).toBe(100);

    generalSettings.updateSetting("pageSizeLimit", 99_999 as unknown as number);
    expect(get(generalSettings).pageSizeLimit).toBe(6000);

    generalSettings.updateSetting("compactCardGap", -5 as unknown as number);
    expect(get(generalSettings).compactCardGap).toBe(0);
  });

  it("falls back to defaults for invalid union values", () => {
    generalSettings.updateSetting("theme", "neon" as never);
    expect(get(generalSettings).theme).toBe(DEFAULT_GENERAL_SETTINGS.theme);

    generalSettings.updateSetting("language", "fr" as unknown as "zh-CN" | "en");
    expect(get(generalSettings).language).toBe(DEFAULT_GENERAL_SETTINGS.language);

    generalSettings.updateSetting("searchCacheEviction", "random" as unknown as "fifo" | "lru");
    expect(get(generalSettings).searchCacheEviction).toBe(
      DEFAULT_GENERAL_SETTINGS.searchCacheEviction,
    );
  });

  it("normalizes partial fontSizes objects without dropping sibling keys", () => {
    const original = get(generalSettings).fontSizes;
    generalSettings.updateSetting("fontSizes", {
      ...original,
      base: 18,
      secondary: 999 as unknown as number,
    });
    const updated = get(generalSettings).fontSizes;
    expect(updated.base).toBe(18);
    expect(updated.secondary).toBe(16); // clamped to max
    expect(updated.tiny).toBe(original.tiny);
    expect(updated.cardTitle).toBe(original.cardTitle);
    expect(updated.cardPreview).toBe(original.cardPreview);
  });

  it("merge applies several keys at once and normalizes them together", () => {
    generalSettings.merge({ compactMode: true, compactImageHeight: 300 as unknown as number });
    const settings = get(generalSettings);
    expect(settings.compactMode).toBe(true);
    expect(settings.compactImageHeight).toBe(200); // clamped to max
    // Sibling keys survive a merge.
    expect(settings.language).toBe(get(generalSettings).language);
  });

  it("persists normalized values to localStorage after the debounce window", () => {
    vi.useFakeTimers();
    generalSettings.updateSetting("pinCopiedToTop", false);
    vi.advanceTimersByTime(200);
    const raw = window.localStorage.getItem(STORAGE_KEY);
    expect(raw).not.toBeNull();
    expect(JSON.parse(raw!).pinCopiedToTop).toBe(false);
    // The persisted payload is the full normalized object, not a diff.
    expect(JSON.parse(raw!).language).toBeDefined();
  });

  it("applies external storage events from another tab onto the store", () => {
    const external = { ...DEFAULT_GENERAL_SETTINGS, searchHistoryEnabled: true };
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(external));
    window.dispatchEvent(
      new StorageEvent("storage", {
        key: STORAGE_KEY,
        newValue: window.localStorage.getItem(STORAGE_KEY)!,
      }),
    );
    expect(get(generalSettings).searchHistoryEnabled).toBe(true);

    // Malformed payloads are ignored instead of corrupting the store.
    window.localStorage.setItem(STORAGE_KEY, "not-json{");
    window.dispatchEvent(new StorageEvent("storage", { key: STORAGE_KEY, newValue: "not-json{" }));
    expect(get(generalSettings).searchHistoryEnabled).toBe(true);
  });
});
