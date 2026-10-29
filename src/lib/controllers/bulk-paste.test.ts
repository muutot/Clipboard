import { mount, unmount, flushSync } from "svelte";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import Probe, { type ProbeState } from "./controllers.probe.svelte";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), write: vi.fn(), toast: vi.fn(), staged: "" }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (path: string) => path,
}));
vi.mock("$lib/services/toast", () => ({ showToast: bridge.toast }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  const { DEFAULT_GENERAL_SETTINGS } = await import("$lib/services/settings/defaults");
  return {
    DEFAULT_GENERAL_SETTINGS,
    generalSettings: writable({ ...DEFAULT_GENERAL_SETTINGS, pasteCleaningEnabled: false }),
  };
});
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  loadClipboardHistory: vi.fn(async () => []),
  loadDeletedClipboardHistory: vi.fn(async () => []),
  searchClipboardHistory: vi.fn(async () => ({ items: [], totalCount: 0, truncated: false })),
}));

let out: ProbeState;
let target: HTMLDivElement;
let app: ReturnType<typeof mount>;
const item = (id: string): ClipboardItem => ({
  id,
  kind: "text",
  title: id,
  preview: id,
  textContent: id,
  sourceApp: "test",
  sourceTone: "neutral",
  sizeLabel: "1 B",
  createdAt: 1,
  favorite: false,
});
beforeEach(() => {
  bridge.invoke.mockReset().mockResolvedValue(true);
  bridge.write.mockReset().mockImplementation(async (text: string) => {
    bridge.staged = text;
  });
  bridge.toast.mockReset();
  bridge.staged = "";
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.stubGlobal("navigator", { clipboard: { writeText: bridge.write } });
  vi.spyOn(console, "error").mockImplementation(() => {});
  out = {};
  target = document.createElement("div");
  document.body.append(target);
  app = mount(Probe, { target, props: { out, initial: [item("a"), item("b")] } });
  flushSync();
});
afterEach(async () => {
  await unmount(app);
  target.remove();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

it.each(["write-error", "target-error", "target-unavailable"])(
  "keeps an undelivered row selected with the real paste service (%s)",
  async (failure) => {
    bridge.write.mockImplementation(async (text: string) => {
      if (text === "a" && failure === "write-error") throw new Error("fixture busy clipboard");
      bridge.staged = text;
    });
    bridge.invoke.mockImplementation(async (command: string) => {
      if (command === "paste_to_previous_application" && bridge.staged === "a") {
        if (failure === "target-error") throw new Error("fixture denied target");
        if (failure === "target-unavailable") return false;
      }
      return true;
    });
    out.bulk!.selectedIds = new Set(["a", "b"]);
    await out.bulk!.bulkPasteAll();
    expect([...out.bulk!.selectedIds]).toEqual(["a"]);
    expect(bridge.toast).not.toHaveBeenCalledWith("toast.bulkPasteSuccess", "success");
    expect(bridge.toast).toHaveBeenCalledWith("toast.pasteFailed", "error");
    expect(bridge.write.mock.calls.map(([text]) => text)).toEqual(["a", "b"]);
  },
);
it("clears delivered rows and reports success with the real paste service", async () => {
  out.bulk!.selectedIds = new Set(["a", "b"]);
  await out.bulk!.bulkPasteAll();
  expect([...out.bulk!.selectedIds]).toEqual([]);
  expect(bridge.toast).toHaveBeenCalledWith("toast.bulkPasteSuccess", "success");
});
