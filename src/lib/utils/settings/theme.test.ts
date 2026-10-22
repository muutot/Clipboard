import { beforeEach, describe, expect, it, vi } from "vitest";
import { DARK_THEME_COLORS, LIGHT_THEME_COLORS } from "$lib/types/clipboard";
import { applyThemeColors, THEME_CACHE_KEY } from "./theme";

function readCache(): Record<string, string> {
  return JSON.parse(window.localStorage.getItem(THEME_CACHE_KEY) ?? "{}");
}

describe("theme first-paint snapshot", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("caches every applied color under its CSS variable", () => {
    applyThemeColors(LIGHT_THEME_COLORS);

    const cached = readCache();
    expect(Object.keys(cached)).toHaveLength(Object.keys(LIGHT_THEME_COLORS).length);
    expect(cached["--bg-app"]).toBe(LIGHT_THEME_COLORS.bg);
    expect(cached["--bg-settings"]).toBe(LIGHT_THEME_COLORS.settingsBg);
    expect(cached["--selection-color"]).toBe(LIGHT_THEME_COLORS.selectionColor);
  });

  it("caches the dark defaults when colors are missing", () => {
    applyThemeColors(undefined);

    expect(readCache()["--bg-app"]).toBe(DARK_THEME_COLORS.bg);
  });

  it("keeps applying colors when storage rejects the write", () => {
    const setItem = vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });

    expect(() => applyThemeColors(LIGHT_THEME_COLORS)).not.toThrow();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);

    setItem.mockRestore();
  });
});
