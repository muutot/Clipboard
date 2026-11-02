import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { DARK_THEME_COLORS, LIGHT_THEME_COLORS } from "$lib/types/clipboard";
import {
  applyThemeColors,
  resolveThemeColors,
  THEME_CACHE_KEY,
  THEME_MODE_CACHE_KEY,
  watchSystemTheme,
} from "./theme";
import { DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings/defaults";
import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
import { normalizeGeneralSettings } from "$lib/services/settings/normalize";
import themeInit from "../../../../static/theme-init.js?raw";

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("system theme defaults", () => {
  beforeEach(() => {
    window.localStorage.clear();
    document.documentElement.removeAttribute("style");
  });

  it("defaults new and missing configurations to system", () => {
    expect(DEFAULT_GENERAL_SETTINGS.theme).toBe("system");
    expect(normalizeGeneralSettings({}).theme).toBe("system");
  });

  it.each([false, true])("follows the system dark preference %s after hydration", (dark) => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: dark })),
    );
    applyGeneralSettingsToDocument(normalizeGeneralSettings({}));
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(
      dark ? DARK_THEME_COLORS.bg : LIGHT_THEME_COLORS.bg,
    );
    expect(document.documentElement.dataset.codeTheme).toBe(dark ? "vs-dark" : "github-light");
    vi.unstubAllGlobals();
  });

  it.each([false, true])("paints a fresh profile before hydration with dark=%s", (dark) => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: dark })),
    );
    new Function(themeInit)();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(
      dark ? DARK_THEME_COLORS.bg : LIGHT_THEME_COLORS.bg,
    );
    vi.unstubAllGlobals();
  });

  it.each([false, true])("re-resolves a system snapshot after restart with dark=%s", (dark) => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: !dark })),
    );
    applyGeneralSettingsToDocument(normalizeGeneralSettings({}));
    expect(window.localStorage.getItem(THEME_MODE_CACHE_KEY)).toBe("system");
    document.documentElement.removeAttribute("style");
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: dark })),
    );
    new Function(themeInit)();
    const expected = dark ? DARK_THEME_COLORS : LIGHT_THEME_COLORS;
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(expected.bg);
    expect(document.documentElement.style.colorScheme).toBe(dark ? "dark" : "light");
    applyThemeColors(expected, "system");
    const vars = JSON.parse(window.localStorage.getItem(THEME_CACHE_KEY)!);
    document.documentElement.removeAttribute("style");
    new Function(themeInit)();
    for (const [name, value] of Object.entries(vars)) {
      expect(document.documentElement.style.getPropertyValue(name)).toBe(value);
    }
  });

  it.each(["light", "dark", "custom"] as const)(
    "retains explicit %s even on the opposite system theme",
    (mode) => {
      vi.stubGlobal(
        "matchMedia",
        vi.fn(() => ({ matches: mode === "light" })),
      );
      const colors =
        mode === "dark" ? DARK_THEME_COLORS : { ...LIGHT_THEME_COLORS, accent: "#123456" };
      const settings = normalizeGeneralSettings({ theme: mode, themeColors: colors });
      expect(settings.theme).toBe(mode);
      applyGeneralSettingsToDocument(settings);
      expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(colors.bg);
      expect(document.documentElement.style.colorScheme).toBe(mode === "dark" ? "dark" : "light");
      document.documentElement.removeAttribute("style");
      new Function(themeInit)();
      expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(colors.bg);
    },
  );

  it("uses a light fallback when no system preference API exists", () => {
    vi.stubGlobal("matchMedia", undefined);
    expect(resolveThemeColors(undefined, "system")).toEqual(LIGHT_THEME_COLORS);
  });

  it("preserves a legacy explicit palette snapshot that has no mode key", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: false })),
    );
    applyThemeColors(DARK_THEME_COLORS);
    window.localStorage.removeItem(THEME_MODE_CACHE_KEY);
    document.documentElement.removeAttribute("style");
    new Function(themeInit)();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(DARK_THEME_COLORS.bg);
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("keeps browser control appearance aligned with a dark custom palette", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: false })),
    );
    applyGeneralSettingsToDocument(
      normalizeGeneralSettings({ theme: "custom", themeColors: DARK_THEME_COLORS }),
    );
    expect(document.documentElement.style.colorScheme).toBe("dark");
    document.documentElement.removeAttribute("style");
    new Function(themeInit)();
    expect(document.documentElement.style.colorScheme).toBe("dark");
  });

  it("uses system appearance despite corrupt or blocked prepaint storage", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: false })),
    );
    window.localStorage.setItem(THEME_CACHE_KEY, "{bad json");
    new Function(themeInit)();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
    document.documentElement.removeAttribute("style");
    vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
      throw new Error("blocked");
    });
    new Function(themeInit)();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
  });

  it("fills a partial explicit-light cache without inheriting system-dark fallbacks", () => {
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => ({ matches: true })),
    );
    window.localStorage.setItem(THEME_MODE_CACHE_KEY, "light");
    window.localStorage.setItem(THEME_CACHE_KEY, JSON.stringify({ "--accent": "#123456" }));
    new Function(themeInit)();
    expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
    expect(document.documentElement.style.getPropertyValue("--accent")).toBe("#123456");
    expect(document.documentElement.style.colorScheme).toBe("light");
  });

  it("normalizes old explicit presets without colors to their own palette", () => {
    expect(normalizeGeneralSettings({ theme: "dark" }).themeColors).toEqual(DARK_THEME_COLORS);
    expect(normalizeGeneralSettings({ theme: "light" }).themeColors).toEqual(LIGHT_THEME_COLORS);
  });

  it("releases the system change listener", () => {
    const media = new EventTarget();
    vi.stubGlobal(
      "matchMedia",
      vi.fn(() => media),
    );
    const callback = vi.fn();
    const stop = watchSystemTheme(callback);
    media.dispatchEvent(new Event("change"));
    expect(callback).toHaveBeenCalledTimes(1);
    stop();
    media.dispatchEvent(new Event("change"));
    expect(callback).toHaveBeenCalledTimes(1);
  });
});

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
