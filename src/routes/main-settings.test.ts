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
