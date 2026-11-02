import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { DEFAULT_GENERAL_SETTINGS } from "./settings/defaults";
import { LIGHT_THEME_COLORS, DARK_THEME_COLORS } from "$lib/types/clipboard";

const native = vi.hoisted(() => ({ label: "main", windowColor: vi.fn(), webviewColor: vi.fn() }));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    get label() {
      return native.label;
    },
    setBackgroundColor: native.windowColor,
  }),
}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ setBackgroundColor: native.webviewColor }),
}));
beforeEach(() => {
  vi.resetModules();
  native.label = "main";
  native.windowColor.mockReset().mockResolvedValue(undefined);
  native.webviewColor.mockReset().mockResolvedValue(undefined);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: false })),
  );
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it("keeps native window and webview backdrops aligned through system changes", async () => {
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  const settings = { ...DEFAULT_GENERAL_SETTINGS, windowTransparency: 100 };
  applyGeneralSettingsToDocument(settings);
  expect(native.windowColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.bg);
  expect(native.webviewColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.bg);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: true })),
  );
  applyGeneralSettingsToDocument(settings);
  expect(native.windowColor).toHaveBeenLastCalledWith(DARK_THEME_COLORS.bg);
  expect(native.webviewColor).toHaveBeenLastCalledWith(DARK_THEME_COLORS.bg);
});

it("leaves transparent main windows without a native erase brush", async () => {
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  for (const overrides of [
    { windowTransparency: 95 },
    { windowOpacityAffectsText: true },
    { windowEffect: "mica" as const },
  ]) {
    applyGeneralSettingsToDocument({
      ...DEFAULT_GENERAL_SETTINGS,
      windowTransparency: 100,
      ...overrides,
    });
  }
  expect(native.windowColor).not.toHaveBeenCalled();
  expect(native.webviewColor).not.toHaveBeenCalled();
});

it("uses the settings backdrop rather than the main background", async () => {
  native.label = "settings";
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  applyGeneralSettingsToDocument(DEFAULT_GENERAL_SETTINGS);
  expect(native.windowColor).toHaveBeenCalledWith(LIGHT_THEME_COLORS.settingsBg);
  expect(native.webviewColor).toHaveBeenCalledWith(LIGHT_THEME_COLORS.settingsBg);
});

it("retains CSS and retries after an optional native backdrop failure", async () => {
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  vi.spyOn(console, "error").mockImplementation(() => {});
  native.windowColor.mockRejectedValueOnce(new Error("native unavailable"));
  const settings = { ...DEFAULT_GENERAL_SETTINGS, windowTransparency: 100 };
  applyGeneralSettingsToDocument(settings);
  for (let i = 0; i < 4; i++) await Promise.resolve();
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
  applyGeneralSettingsToDocument(settings);
  expect(native.windowColor).toHaveBeenCalledTimes(2);
});
