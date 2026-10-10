import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { pasteClipboardItem } from "./clipboard";
import { t } from "$lib/i18n";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  writeText: vi.fn(),
  write: vi.fn(),
  toast: vi.fn(),
  cleaning: true,
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (x: string) => x,
}));
vi.mock("$lib/services/toast", () => ({ showToast: bridge.toast }));
vi.mock("$lib/services/log", () => ({ logFrontendError: vi.fn(), logFrontendMessage: vi.fn() }));
vi.mock("$lib/services/settings", () => ({
  generalSettings: {
    subscribe(run: (value: unknown) => void) {
      run({ pinCopiedToTop: false, pasteCleaningEnabled: bridge.cleaning });
      return () => {};
    },
  },
}));

const item = {
  id: "record",
  kind: "text",
  title: "hello",
  textContent: "  hello   world?utm=1 ",
  htmlContent: "<b>hello world</b>",
} as ClipboardItem;

beforeEach(() => {
  bridge.cleaning = true;
  bridge.invoke.mockReset().mockResolvedValue(true);
  bridge.writeText.mockReset().mockResolvedValue(undefined);
  bridge.write.mockReset().mockResolvedValue(undefined);
  bridge.toast.mockReset();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.stubGlobal("navigator", { clipboard: { writeText: bridge.writeText, write: bridge.write } });
  vi.stubGlobal(
    "ClipboardItem",
    class {
      constructor(public readonly data: unknown) {}
    },
  );
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

function invokeMap(transformResult: string | null) {
  return async (command: string) => {
    if (command === "transform_text") {
      return { input: item.textContent, operation: "cleanPaste", result: transformResult };
    }
    return true;
  };
}

describe("format paste with cleaning", () => {
  it("reports the cleaned plain-text downgrade instead of claiming the format", async () => {
    bridge.invoke.mockImplementation(invokeMap("hello world"));
    await pasteClipboardItem(item, "format");
    expect(bridge.writeText).toHaveBeenCalledWith("hello world");
    expect(bridge.write).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(t("toast.formatPasteCleanedSuccess"), "success");
  });

  it("keeps rich HTML when cleaning does not change the text", async () => {
    bridge.invoke.mockImplementation(invokeMap(item.textContent ?? null));
    await pasteClipboardItem(item, "format");
    expect(bridge.write).toHaveBeenCalledTimes(1);
    expect(bridge.writeText).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(t("toast.formatPasteSuccess"), "success");
  });

  it("writes rich HTML when cleaning is disabled", async () => {
    bridge.cleaning = false;
    bridge.invoke.mockImplementation(invokeMap("hello world"));
    await pasteClipboardItem(item, "format");
    expect(bridge.write).toHaveBeenCalledTimes(1);
    expect(bridge.writeText).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(t("toast.formatPasteSuccess"), "success");
  });
});
