import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import type { ClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
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
