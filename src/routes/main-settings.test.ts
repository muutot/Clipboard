import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { DEFAULT_GENERAL_SETTINGS, generalSettings } from "$lib/services/settings";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  search: vi.fn(),
  history: vi.fn(),
  desktop: false,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name, callback) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    label: "main",
    setAlwaysOnTop: async () => {},
    setDecorations: async () => {},
    onMoved: async () => () => {},
    onResized: async () => () => {},
    onFocusChanged: async () => () => {},
  }),
}));
vi.mock("$lib/services/runtime", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/services/runtime")>()),
  isTauriRuntime: () => bridge.desktop,
}));
vi.mock("$lib/services/clipboard", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/services/clipboard")>()),
  searchClipboardHistory: bridge.search,
  loadClipboardHistory: bridge.history,
}));

function item(id: string): ClipboardItem {
  return {
    id,
    kind: "text",
    title: id,
    preview: id,
    textContent: id,
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "1 B",
    createdAt: 1,
    lastUsedAtMs: 1,
    favorite: false,
  };
}
function persisted(id: string): PersistedClipboardItem {
  return {
    id,
    kind: "text",
    title: id,
    textContent: id,
    contentHash: id,
    sourceApp: "test",
    sizeBytes: 1,
    createdAtMs: 1,
    lastUsedAtMs: 2,
    isFavorite: false,
  } as PersistedClipboardItem;
}
const page = (...ids: string[]) => ({
  items: ids.map(item),
  totalCount: ids.length,
  truncated: false,
});
let app: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;
async function settle(ms = 0) {
  flushSync();
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
    flushSync();
  }
}
async function open(query = "query") {
  target = document.createElement("div");
  document.body.append(target);
  app = mount(MainPage, { target });
  await settle();
  if (query) {
    const input = target.querySelector(".search-input") ?? target.querySelector("input");
    (input as HTMLInputElement).value = query;
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle(350);
  }
}
const ids = () =>
  [...target.querySelectorAll<HTMLElement>("[data-id]")].map((row) => row.dataset.id);
const emit = (name: string, payload: unknown = {}) => bridge.listeners.get(name)?.({ payload });
async function scrollToBottom() {
  const list = target.querySelector(".history-list")!;
  list.dispatchEvent(new Event("scroll"));
  await settle(30);
}
beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    measureText: (text: string) => ({ width: text.length * 7 }),
  } as unknown as CanvasRenderingContext2D);
  vi.useFakeTimers();
  bridge.desktop = false;
  bridge.listeners.clear();
  bridge.search.mockReset().mockResolvedValue(page("a", "b"));
  bridge.history.mockReset().mockResolvedValue([]);
  generalSettings.set(structuredClone(DEFAULT_GENERAL_SETTINGS));
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
});
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  target?.remove();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("search settings and live changes", () => {
  it("rebuilds the cursor when a smaller cap trims an in-flight page", async () => {
    bridge.desktop = true;
    const settings = {
      ...structuredClone(DEFAULT_GENERAL_SETTINGS),
      pageSizeLimit: 4,
      loadTolerance: 0,
      display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 4 },
    };
    generalSettings.set(settings);
    let finish!: (items: ClipboardItem[]) => void;
    bridge.history
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      )
      .mockResolvedValue([]);
    await open("");
    generalSettings.set({ ...settings, pageSizeLimit: 2 });
    await settle();
    finish([item("a"), item("b"), item("c"), item("d")]);
    await settle();
    generalSettings.set({ ...settings, pageSizeLimit: 5 });
    await settle(30);
    expect(bridge.history.mock.calls[1][2].cursor).toBeNull();
  });
  it("resumes at the existing cursor after the history cap grows", async () => {
    bridge.desktop = true;
    generalSettings.set({
      ...structuredClone(DEFAULT_GENERAL_SETTINGS),
      pageSizeLimit: 2,
      loadTolerance: 0,
      display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 2 },
    });
    bridge.history.mockResolvedValueOnce([item("a"), item("b")]).mockResolvedValue([item("c")]);
    await open("");
    await scrollToBottom();
    expect(bridge.history).toHaveBeenCalledTimes(1);
    generalSettings.set({
      ...structuredClone(DEFAULT_GENERAL_SETTINGS),
      pageSizeLimit: 3,
      loadTolerance: 0,
      display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 2 },
    });
    await settle(30);
    expect(bridge.history).toHaveBeenCalledTimes(2);
    expect(bridge.history.mock.calls[1][2].cursor).toEqual({ lastUsedAtMs: 1, id: "b" });
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("checks exhaustion against the requested page size during a settings change", async () => {
    bridge.desktop = true;
    generalSettings.set({
      ...structuredClone(DEFAULT_GENERAL_SETTINGS),
      display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 2 },
    });
    let finish!: (items: ClipboardItem[]) => void;
    bridge.history
      .mockImplementationOnce(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      )
      .mockResolvedValue([]);
    await open("");
    generalSettings.set({
      ...structuredClone(DEFAULT_GENERAL_SETTINGS),
      display: { ...DEFAULT_GENERAL_SETTINGS.display, pageSize: 3 },
    });
    finish([item("a"), item("b")]);
    await settle();
    await scrollToBottom();
    expect(bridge.history).toHaveBeenCalledTimes(2);
    expect(bridge.history.mock.calls[1][0]).toBe(3);
  });
  it("refreshes when the candidate cap changes without resetting for unrelated settings", async () => {
    await open();
    bridge.search.mockClear();
    generalSettings.updateSetting("showToastNotifications", false);
    await settle(350);
    expect(bridge.search).not.toHaveBeenCalled();
    generalSettings.updateSetting("searchPageSizeLimit", 200);
    await settle(350);
    expect(bridge.search).toHaveBeenCalledTimes(1);
  });
  it("preserves title sorting during recapture and then refreshes matching results", async () => {
    generalSettings.updateSetting("searchSortRules", [{ field: "title", direction: "asc" }]);
    await open();
    expect(ids()).toEqual(["a", "b"]);
    bridge.search.mockResolvedValue(page("a", "b", "c"));
    emit("clipboard-item-added", persisted("b"));
    await settle();
    expect(ids()).toEqual(["a", "b"]);
    await settle(350);
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("refreshes recent-use sorting and rejects an older in-flight response", async () => {
    let finish!: (value: ReturnType<typeof page>) => void;
    bridge.search.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await open();
    bridge.search.mockResolvedValue(page("b", "a"));
    emit("clipboard-items-changed", {
      usedIds: ["b"],
      usageUpdates: [{ id: "b", lastUsedAtMs: 10 }],
    });
    await settle(350);
    expect(ids()).toEqual(["b", "a"]);
    finish(page("stale"));
    await settle();
    expect(ids()).toEqual(["b", "a"]);
  });

  it("refreshes after background index commits but not for irrelevant usage", async () => {
    generalSettings.updateSetting("searchSortRules", [{ field: "title", direction: "asc" }]);
    await open();
    bridge.search.mockClear();
    emit("clipboard-items-changed", { usedIds: ["b"] });
    await settle(350);
    expect(bridge.search).not.toHaveBeenCalled();
    expect(ids()).toEqual(["a", "b"]);
    bridge.search.mockResolvedValue(page("a", "b", "new"));
    emit("search-index-changed");
    await settle(350);
    expect(ids()).toEqual(["a", "b", "new"]);
  });
});

describe("complete history search", () => {
  it.each(["账", "昨天"])("queries the backend for %s beyond loaded history", async (query) => {
    bridge.desktop = true;
    const remote = { ...item("outside-loaded-history"), createdAt: Date.now() - 86400000 };
    bridge.search.mockResolvedValue({ items: [remote], totalCount: 1, truncated: false });
    await open(query);
    expect(bridge.search).toHaveBeenCalledWith(
      query,
      expect.any(Number),
      0,
      expect.any(Array),
      expect.any(Object),
    );
    expect(ids()).toEqual([remote.id]);
  });
});
