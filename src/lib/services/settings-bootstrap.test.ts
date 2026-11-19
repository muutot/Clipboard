import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { DEFAULT_GENERAL_SETTINGS } from "./settings/defaults";
import { LIGHT_THEME_COLORS, DARK_THEME_COLORS } from "#lib/types/clipboard.ts";

const native = vi.hoisted(() => ({ label: "main", windowColor: vi.fn(), webviewColor: vi.fn() }));
vi.mock("#lib/services/runtime.ts", () => ({ isTauriRuntime: () => true }));
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

it("never rewrites the main window's native backdrop", async () => {
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  // The main window's native surfaces are decided once, at creation, in
  // `lib.rs`. Writing a WebView2 background from the page is irreversible
  // (`setBackgroundColor(null)` falls back to opaque white), so any write here
  // would freeze the window opaque and make the transparency slider inert until
  // a restart. Cover the full range that used to trigger a write.
  for (const overrides of [
    { windowTransparency: 100 },
    { windowTransparency: 67 },
    { windowOpacityAffectsText: true },
    { windowEffect: "mica" as const },
  ]) {
    applyGeneralSettingsToDocument({ ...DEFAULT_GENERAL_SETTINGS, ...overrides });
  }
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: true })),
  );
  applyGeneralSettingsToDocument({ ...DEFAULT_GENERAL_SETTINGS, windowTransparency: 100 });
  for (let i = 0; i < 4; i++) await Promise.resolve();
  expect(native.windowColor).not.toHaveBeenCalled();
  expect(native.webviewColor).not.toHaveBeenCalled();
});

it("keeps the settings backdrop aligned through system changes", async () => {
  native.label = "settings";
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  applyGeneralSettingsToDocument(DEFAULT_GENERAL_SETTINGS);
  expect(native.windowColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.settingsBg);
  expect(native.webviewColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.settingsBg);
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => ({ matches: true })),
  );
  applyGeneralSettingsToDocument(DEFAULT_GENERAL_SETTINGS);
  expect(native.windowColor).toHaveBeenLastCalledWith(DARK_THEME_COLORS.settingsBg);
  expect(native.webviewColor).toHaveBeenLastCalledWith(DARK_THEME_COLORS.settingsBg);
});

it("uses the main background for the float panel", async () => {
  native.label = "float";
  const { applyGeneralSettingsToDocument } = await import("./settings-bootstrap");
  applyGeneralSettingsToDocument(DEFAULT_GENERAL_SETTINGS);
  expect(native.windowColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.bg);
  expect(native.webviewColor).toHaveBeenLastCalledWith(LIGHT_THEME_COLORS.bg);
});

it("retains CSS and retries after an optional native backdrop failure", async () => {
  native.label = "settings";
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
