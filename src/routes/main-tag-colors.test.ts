import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";

// Tag colors live in the backend tag registry, and the main window learns
// about a change through the `tags-changed` event. A color-only change
// carries an empty payload (no record was renamed or deleted), so the route
// must still refresh the colors — otherwise every chip keeps the old accent
// until the app restarts.

type EventHandler = (event: { payload: unknown }) => void;

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, EventHandler>(),
  listAllTags: vi.fn(async () => [{ name: "work", count: 1, color: "#ff0000" }]),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (event: string, handler: EventHandler) => {
    bridge.listeners.set(event, handler);
    return () => bridge.listeners.delete(event);
  }),
}));

vi.mock("$lib/services/clipboard", async (importOriginal) => {
  const actual = await importOriginal<typeof import("$lib/services/clipboard")>();
  return { ...actual, listAllTags: bridge.listAllTags };
});

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.listeners.clear();
  bridge.listAllTags.mockClear();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

describe("main window tag color refresh", () => {
  it("re-reads the tag colors when only a color changed", async () => {
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
    await settle();

    const before = bridge.listAllTags.mock.calls.length;
    expect(before).toBeGreaterThan(0);

    const handler = bridge.listeners.get("tags-changed");
    expect(handler).toBeTypeOf("function");

    // The color-only payload: no rename, no delete.
    handler!({ payload: {} });
    await settle();

    expect(bridge.listAllTags.mock.calls.length).toBe(before + 1);
  });
});
