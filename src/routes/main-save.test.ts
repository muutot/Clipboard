import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { t } from "$lib/i18n";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), save: vi.fn(), toast: vi.fn() }));
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  materializeClipboardItem: async (item: ClipboardItem) => item,
}));
vi.mock("@tauri-apps/api/core", async (original) => ({
  ...(await original<typeof import("@tauri-apps/api/core")>()),
  invoke: bridge.invoke,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ save: bridge.save }));
vi.mock("$lib/data/demo-items", async (original) => {
  const source = await original<typeof import("$lib/data/demo-items")>();
  return {
    ...source,
    demoClipboardItems: source.demoClipboardItems.map((item) =>
      item.kind === "image" || item.kind === "file"
        ? { ...item, resourcePath: "C:/synthetic/local.bin" }
        : item,
    ),
  };
});
vi.mock("$lib/services/toast", async (original) => ({
  ...(await original<typeof import("$lib/services/toast")>()),
  showToast: bridge.toast,
}));
let app: ReturnType<typeof mount>;
let target: HTMLDivElement;
async function settle() {
  for (let i = 0; i < 24; i++) {
    await Promise.resolve();
    flushSync();
  }
}
beforeEach(async () => {
  vi.clearAllMocks();
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    measureText: (text: string) => ({ width: text.length * 7 }),
  } as unknown as CanvasRenderingContext2D);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  target = document.createElement("div");
  document.body.append(target);
  app = mount(MainPage, { target });
  await settle();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
});
afterEach(async () => {
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  await unmount(app);
  target.remove();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
it.each(["external", "missing"])(
  "explains %s before opening a destination dialog",
  async (capability) => {
    bridge.invoke.mockResolvedValue(capability);
    const button = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.getAttribute("aria-label") === t("card.saveAs"),
    );
    expect(button).toBeDefined();
    button!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith("get_clipboard_item_save_capability", {
      id: expect.any(String),
    });
    expect(bridge.save).not.toHaveBeenCalled();
    expect(bridge.toast).toHaveBeenCalledWith(
      t(capability === "external" ? "toast.saveExternalUnsupported" : "toast.saveSourceMissing"),
      "error",
    );
  },
);
it("saves an available record only after preflight and destination selection", async () => {
  bridge.invoke.mockResolvedValue("available");
  bridge.save.mockResolvedValue("C:/synthetic/export.bin");
  [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.getAttribute("aria-label") === t("card.saveAs"))!
    .click();
  await settle();
  await vi.waitFor(() => {
    expect(bridge.save).toHaveBeenCalledOnce();
    expect(bridge.invoke).toHaveBeenCalledWith("save_clipboard_item_file", {
      id: expect.any(String),
      dst: "C:/synthetic/export.bin",
    });
  });
});
