import { mount, unmount, flushSync } from "svelte";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import Probe, { type ProbeState } from "./controllers.probe.svelte";
import { appendItems } from "$lib/utils/store/item-store";
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
  bridge.invoke
    .mockReset()
    .mockImplementation(async (command: string, args?: { text?: string }) => {
      if (command === "write_clipboard_text") {
        bridge.staged = args?.text ?? "";
        return;
      }
      return command === "begin_paste_batch" ? "batch" : true;
    });
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
    bridge.invoke.mockImplementation(async (command: string, args?: { text?: string }) => {
      if (command === "write_clipboard_text") {
        if (args?.text === "a" && failure === "write-error")
          throw new Error("fixture busy clipboard");
        bridge.staged = args?.text ?? "";
        return;
      }
      if (command === "paste_to_previous_application" && bridge.staged === "a") {
        if (failure === "target-error") throw new Error("fixture denied target");
        if (failure === "target-unavailable") return false;
      }
      return command === "begin_paste_batch" ? "batch" : true;
    });
    out.bulk!.selectedIds = new Set(["a", "b"]);
    await out.bulk!.bulkPasteAll();
    expect([...out.bulk!.selectedIds]).toEqual(["a", "b"]);
    expect(bridge.toast).not.toHaveBeenCalledWith("toast.bulkPasteSuccess", "success");
    expect(bridge.toast).toHaveBeenCalledWith("toast.pasteFailed", "error");
    expect(
      bridge.invoke.mock.calls
        .filter(([command]) => command === "write_clipboard_text")
        .map(([, args]) => (args as { text?: string } | undefined)?.text),
    ).toEqual(["a"]);
  },
);
it("clears delivered rows and reports success with the real paste service", async () => {
  out.bulk!.selectedIds = new Set(["a", "b"]);
  await out.bulk!.bulkPasteAll();
  expect([...out.bulk!.selectedIds]).toEqual([]);
  expect(bridge.toast).toHaveBeenCalledWith("toast.bulkPasteSuccess", "success");
});

it("takes the previous target once and delivers three rows through one lease", async () => {
  out.store!.current = appendItems(out.store!.current, [item("c")], "history");
  out.bulk!.selectedIds = new Set(["a", "b", "c"]);
  let remembered: string | null = "editor";
  let active: string | null = null;
  const delivered: string[] = [];
  bridge.invoke.mockImplementation(
    async (command: string, args?: { batchToken?: string; token?: string; text?: string }) => {
      if (command === "write_clipboard_text") {
        bridge.staged = args?.text ?? "";
        return;
      }
      if (command === "begin_paste_batch") {
        active = remembered;
        remembered = null;
        return active ? "lease" : null;
      }
      if (command === "validate_paste_batch") {
        if (!active || args?.token !== "lease") throw Error("expired");
        return;
      }
      if (command === "paste_to_previous_application") {
        if (!active || args?.batchToken !== "lease") return false;
        delivered.push(bridge.staged);
        return true;
      }
      if (command === "end_paste_batch") {
        active = null;
        return;
      }
      return true;
    },
  );
  await out.bulk!.bulkPasteAll();
  expect(delivered).toEqual(["a", "b", "c"]);
  expect([...out.bulk!.selectedIds]).toEqual([]);
  expect(bridge.invoke.mock.calls.filter(([cmd]) => cmd === "begin_paste_batch")).toHaveLength(1);
  expect(active).toBeNull();
});

it("stops before overwriting the clipboard after batch focus is lost", async () => {
  out.store!.current = appendItems(out.store!.current, [item("c")], "history");
  out.bulk!.selectedIds = new Set(["a", "b", "c"]);
  let delivered = 0;
  bridge.invoke.mockImplementation(async (command: string, args?: { text?: string }) => {
    if (command === "write_clipboard_text") {
      bridge.staged = args?.text ?? "";
      return;
    }
    if (command === "begin_paste_batch") return "lease";
    if (command === "validate_paste_batch" && delivered) throw Error("focus lost");
    if (command === "paste_to_previous_application") {
      delivered++;
      return true;
    }
    return true;
  });
  await out.bulk!.bulkPasteAll();
  expect(
    bridge.invoke.mock.calls
      .filter(([command]) => command === "write_clipboard_text")
      .map(([, args]) => (args as { text?: string } | undefined)?.text),
  ).toEqual(["a"]);
  expect([...out.bulk!.selectedIds]).toEqual(["b", "c"]);
  expect(bridge.invoke).toHaveBeenCalledWith("end_paste_batch", { token: "lease" });
});

it("keeps all rows and does not write when no previous target can be leased", async () => {
  out.bulk!.selectedIds = new Set(["a", "b"]);
  bridge.invoke.mockImplementation(async (command: string) =>
    command === "begin_paste_batch" ? null : true,
  );
  await out.bulk!.bulkPasteAll();
  expect(
    bridge.invoke.mock.calls.filter(([command]) => command === "write_clipboard_text"),
  ).toHaveLength(0);
  expect([...out.bulk!.selectedIds]).toEqual(["a", "b"]);
});
