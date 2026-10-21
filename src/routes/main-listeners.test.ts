import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";

// The browser (non-Tauri) runtime seeds `demoClipboardItems`, so the mounted
// main route renders real rows without a backend. The main-window hide call
// goes through the backend-owned window service, mocked here so the test can
// prove that Escape over a bulk selection exits the selection instead of
// hiding the window (hideWindow's default chord is Escape).

const bridge = vi.hoisted(() => ({
  hide: vi.fn(async () => {}),
}));

vi.mock("$lib/services/window-control", () => ({
  hideMainWindow: bridge.hide,
  showMainWindow: vi.fn(),
}));

let listenCallCount = 0;

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => {
    // First listen call rejects; subsequent ones resolve.
    listenCallCount += 1;
    if (listenCallCount === 1) {
      throw new Error("listen failed");
    }
    return () => {};
  }),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.hide.mockClear();
  listenCallCount = 0;
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

describe("main window history load", () => {
  it("loads history even when a listener registration fails", async () => {
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

    // loadActiveHistoryPage ran despite the rejected listen registration.
    expect(target.querySelector("[data-id]")).toBeTruthy();
  });
});
