import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { copyClipboardItem, pasteClipboardItem, hydrateClipboardItem } from "./clipboard";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), write: vi.fn(), toast: vi.fn(), pin: true }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (x: string) => x,
}));
vi.mock("$lib/services/toast", () => ({ showToast: bridge.toast }));
vi.mock("$lib/services/log", () => ({ logFrontendError: vi.fn(), logFrontendMessage: vi.fn() }));
vi.mock("$lib/services/settings", () => ({
  generalSettings: {
    subscribe(run: (value: unknown) => void) {
      run({ pinCopiedToTop: bridge.pin, pasteCleaningEnabled: false });
      return () => {};
    },
  },
}));

const item = { id: "record", kind: "text", title: "hello", textContent: "hello" } as ClipboardItem;
beforeEach(() => {
  bridge.pin = true;
  bridge.invoke.mockReset().mockResolvedValue(true);
  bridge.write.mockReset().mockResolvedValue(undefined);
  bridge.toast.mockReset();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.stubGlobal("navigator", { clipboard: { writeText: bridge.write } });
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
const stamps = () =>
  bridge.invoke.mock.calls.filter(([command]) => command === "set_clipboard_item_last_used");

describe("successful usage recording", () => {
  it.each(["copy", "paste"])(
    "hydrates complete text before %s and never writes a preview",
    async (action) => {
      const full = "complete body ".repeat(1000);
      bridge.invoke.mockImplementation(async (command) =>
        command === "get_clipboard_item"
          ? {
              ...item,
              textContent: full,
              createdAtMs: 1,
              sizeBytes: full.length,
              isFavorite: false,
            }
          : true,
      );
      const summary = { ...item, contentLoaded: false, textContent: "preview" };
      if (action === "copy") await copyClipboardItem(summary);
      else await pasteClipboardItem(summary, "plain");
      if (action === "copy") {
        expect(bridge.invoke).toHaveBeenCalledWith("copy_clipboard_item", { id: item.id });
        expect(bridge.write).not.toHaveBeenCalled();
      } else expect(bridge.write).toHaveBeenCalledWith(full);
    },
  );
  it("coalesces concurrent hydration and does not retain stale bodies after it completes", async () => {
    let finish!: (value: unknown) => void;
    bridge.invoke.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    const summary = { ...item, contentLoaded: false, textContent: "preview" };
    const a = hydrateClipboardItem(summary);
    const b = hydrateClipboardItem(summary);
    expect(bridge.invoke).toHaveBeenCalledTimes(1);
    finish({ ...item, textContent: "full", sizeBytes: 4, createdAtMs: 1, isFavorite: false });
    expect((await a).textContent).toBe("full");
    expect((await b).textContent).toBe("full");
    bridge.invoke.mockResolvedValue(null);
    await expect(hydrateClipboardItem(summary)).rejects.toThrow("no longer exists");
  });
  it("never copies a truncated preview if hydration fails", async () => {
    bridge.invoke.mockRejectedValue(new Error("unavailable"));
    await copyClipboardItem({ ...item, contentLoaded: false });
    expect(bridge.write).not.toHaveBeenCalled();
    expect(stamps()).toHaveLength(0);
  });
  it("waits for the OS write before stamping and moving a copied row", async () => {
    let finish!: () => void;
    bridge.invoke.mockImplementation(
      () =>
        new Promise<boolean>((resolve) => {
          finish = () => resolve(true);
        }),
    );
    const moveToTop = vi.fn();
    const copy = copyClipboardItem(item, { moveToTop });
    await vi.waitFor(() =>
      expect(bridge.invoke).toHaveBeenCalledWith("copy_clipboard_item", { id: item.id }),
    );
    expect(stamps()).toHaveLength(0);
    expect(moveToTop).not.toHaveBeenCalled();
    finish();
    await copy;
    expect(stamps()).toHaveLength(0);
    expect(bridge.invoke).toHaveBeenCalledWith("copy_clipboard_item", { id: item.id });
    expect(moveToTop).toHaveBeenCalledWith(item.id);
  });

  it.each(["copy", "paste"])("does not stamp or move after a failed %s write", async (action) => {
    bridge.write.mockRejectedValue(new Error("busy clipboard"));
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "copy_clipboard_item") throw new Error("busy clipboard");
      return true;
    });
    const moveToTop = vi.fn();
    if (action === "copy") await copyClipboardItem(item, { moveToTop });
    else await pasteClipboardItem(item, "plain", { moveToTop });
    expect(stamps()).toHaveLength(0);
    expect(moveToTop).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(expect.any(String), "error");
  });

  it("does not stamp a file copy whose source disappeared", async () => {
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "materialize_clipboard_item")
        return {
          ...item,
          kind: "file",
          createdAtMs: 1,
          sizeBytes: 1,
          isFavorite: false,
          contentHash: "file",
          resourcePath: "C:/missing.txt",
        };
      if (command === "copy_clipboard_item") throw { kind: "resource-missing" };
      return true;
    });
    const moveToTop = vi.fn();
    await copyClipboardItem({ ...item, kind: "file" }, { moveToTop });
    expect(stamps()).toHaveLength(0);
    expect(moveToTop).not.toHaveBeenCalled();
  });

  it("stamps successful copies even when immediate promotion is off", async () => {
    bridge.pin = false;
    const moveToTop = vi.fn();
    await copyClipboardItem(item, { moveToTop });
    expect(stamps()).toHaveLength(0);
    expect(bridge.invoke).toHaveBeenCalledWith("copy_clipboard_item", { id: item.id });
    expect(moveToTop).not.toHaveBeenCalled();
  });

  it("keeps a successful OS copy successful when metadata persistence fails", async () => {
    bridge.invoke.mockImplementation(async (command) => {
      if (command === "copy_clipboard_item") return false;
      return true;
    });
    const moveToTop = vi.fn();
    await copyClipboardItem(item, { moveToTop });
    expect(moveToTop).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(expect.any(String), "success");
  });
});
