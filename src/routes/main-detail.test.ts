import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  hydrate: vi.fn(),
}));
vi.mock("$lib/services/clipboard", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/services/clipboard")>()),
  hydrateClipboardItem: bridge.hydrate,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name, callback) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.listeners.clear();
  vi.unstubAllGlobals();
});

describe("main window detail requests", () => {
  it("loads a summary before showing its complete detail", async () => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(MainPage, { target });
    cleanups.push(async () => {
      await unmount(app);
      target.remove();
    });
    flushSync();
    const summary = {
      id: "summary",
      kind: "text",
      title: "Large record",
      textContent: "preview",
      contentLoaded: false,
      preview: "",
      sourceApp: "test",
      sourceTone: "neutral",
      sizeLabel: "4 KB",
      createdAt: 0,
      favorite: false,
    } as ClipboardItem;
    let finish!: (item: ClipboardItem) => void;
    bridge.hydrate.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    bridge.listeners.get("clipboard-open-detail")?.({ payload: summary });
    flushSync();
    expect(target.querySelector(".detail-panel")).toBeNull();
    finish({ ...summary, textContent: "FULL CONTENT", contentLoaded: true });
    for (let i = 0; i < 12; i++) {
      await Promise.resolve();
      flushSync();
    }
    expect(target.querySelector(".detail-panel")?.textContent).toContain("FULL CONTENT");
  });
  it("opens a float-only record without inserting it into the history list", async () => {
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(MainPage, { target });
    cleanups.push(async () => {
      await unmount(app);
      target.remove();
    });
    flushSync();
    const item: ClipboardItem = {
      id: "float-only-record",
      kind: "text",
      title: "Float-only detail title",
      preview: "Float-only content",
      textContent: "Float-only content",
      sourceApp: "test",
      sourceTone: "neutral",
      sizeLabel: "18 chars",
      createdAt: 0,
      favorite: false,
    };
    bridge.listeners.get("clipboard-open-detail")?.({ payload: item });
    for (let i = 0; i < 8; i++) {
      await Promise.resolve();
      flushSync();
    }
    expect(target.querySelector(".detail-panel")?.textContent).toContain(item.title);
    expect(target.querySelector('[data-id="float-only-record"]')).toBeNull();
  });
});
