import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { DEFAULT_GENERAL_SETTINGS, generalSettings } from "$lib/services/settings";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  search: vi.fn(),
  history: vi.fn(),
  hydrate: vi.fn(),
  report: vi.fn(async () => {}),
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
  hydrateClipboardItem: bridge.hydrate,
}));
vi.mock("$lib/services/storage", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/services/storage")>()),
  recordSearchInteractionLatency: bridge.report,
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
  } as ClipboardItem;
}

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

function searchInput(): HTMLInputElement {
  return (target.querySelector(".search-input") ??
    target.querySelector("input")) as HTMLInputElement;
}

beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    measureText: (text: string) => ({ width: text.length * 7 }),
  } as unknown as CanvasRenderingContext2D);
  vi.useFakeTimers();
  bridge.desktop = false;
  bridge.listeners.clear();
  bridge.search.mockReset().mockResolvedValue({
    items: [item("a")],
    totalCount: 1,
    truncated: false,
  });
  bridge.history.mockReset().mockResolvedValue([]);
  bridge.hydrate.mockReset().mockImplementation(async (value) => value);
  bridge.report.mockClear();
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
});

async function mountPage() {
  target = document.createElement("div");
  document.body.append(target);
  app = mount(MainPage, { target });
  await settle();
  bridge.desktop = true;
}

describe("search input latency samples", () => {
  it("records a sample when the query changes in the search box", async () => {
    await mountPage();
    const input = searchInput();
    input.focus();
    input.value = "hello";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle(400);
    await settle(64);
    await settle(64);
    expect(bridge.search).toHaveBeenCalled();
    expect(bridge.report).toHaveBeenCalledTimes(1);
  });

  it("records a sample when the query is typed outside the search box", async () => {
    await mountPage();
    document.body.dispatchEvent(
      new KeyboardEvent("keydown", { key: "a", bubbles: true, cancelable: true }),
    );
    await settle(400);
    await settle(64);
    await settle(64);
    expect(bridge.search).toHaveBeenCalled();
    expect(bridge.report).toHaveBeenCalledTimes(1);
  });

  it("records one sample per query change", async () => {
    await mountPage();
    const input = searchInput();
    input.focus();
    input.value = "ab";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle(400);
    await settle(64);
    await settle(64);
    expect(bridge.report).toHaveBeenCalledTimes(1);
    input.value = "abc";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle(400);
    await settle(64);
    await settle(64);
    expect(bridge.report).toHaveBeenCalledTimes(2);
  });
});
