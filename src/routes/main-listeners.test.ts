import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import type { ClipboardItem } from "$lib/types/clipboard";

// Mount the real route with controlled native registrations and history IPC.

const bridge = vi.hoisted(() => ({
  hide: vi.fn(async () => {}),
  native: false,
  registration: undefined as Promise<void> | undefined,
  unlisten: vi.fn(),
  active: vi.fn(async (): Promise<ClipboardItem[]> => []),
  deleted: vi.fn(async () => []),
}));

vi.mock("$lib/services/runtime", async (original) => ({
  ...(await original<typeof import("$lib/services/runtime")>()),
  isTauriRuntime: () => bridge.native,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => null),
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => null }));
vi.mock("$lib/services/settings-bootstrap", async (original) => ({
  ...(await original<typeof import("$lib/services/settings-bootstrap")>()),
  applyGeneralSettingsToDocument: () => {},
}));
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  loadClipboardHistory: bridge.active,
  loadDeletedClipboardHistory: bridge.deleted,
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
    if (bridge.registration) {
      await bridge.registration;
      return bridge.unlisten;
    }
    if (listenCallCount === 1) {
      throw new Error("listen failed");
    }
    return () => {};
  }),
}));

const cleanups: (() => Promise<void>)[] = [];
beforeEach(() => {
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue(null);
});
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.hide.mockClear();
  listenCallCount = 0;
  bridge.native = false;
  bridge.registration = undefined;
  bridge.unlisten.mockClear();
  bridge.active.mockReset().mockResolvedValue([]);
  bridge.deleted.mockClear();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

describe("main window history load", () => {
  it("does not start history reads when registration completes after unmount", async () => {
    bridge.native = true;
    let register!: () => void;
    bridge.registration = new Promise<void>((resolve) => (register = resolve));
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
    flushSync();
    await settle();
    expect(bridge.active).not.toHaveBeenCalled();
    expect(bridge.deleted).not.toHaveBeenCalled();
    const registrations = listenCallCount;
    await unmount(app);
    target.remove();
    register();
    await settle();
    expect(bridge.unlisten).toHaveBeenCalledTimes(registrations);
    expect(bridge.active).not.toHaveBeenCalled();
    expect(bridge.deleted).not.toHaveBeenCalled();
  });

  it("loads history even when a listener registration fails", async () => {
    bridge.native = true;
    bridge.active.mockResolvedValue([
      {
        id: "native-history",
        kind: "text",
        title: "Loaded history",
        preview: "Loaded history",
        sourceApp: "test",
        sourceTone: "neutral",
        sizeLabel: "1 B",
        createdAt: 1,
        lastUsedAtMs: 1,
        favorite: false,
      },
    ]);
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
    expect(bridge.active).toHaveBeenCalledOnce();
    expect(bridge.deleted).toHaveBeenCalledOnce();
    expect(target.querySelector('[data-id="native-history"]')).toBeTruthy();
  });
});
