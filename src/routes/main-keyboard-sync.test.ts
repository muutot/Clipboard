import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import type { KeyboardConfig } from "#lib/services/keyboard.ts";

// The settings window saves bindings through the backend, which broadcasts
// `keyboard-config-changed`. The main window used to re-read keyboard.json
// only on focus, so hovering a toolbar tab from the settings window showed
// the previous chord. These tests mount the real route and prove the
// broadcast updates the tooltip immediately, shows only the first binding of
// a multi-chord action, and cannot be overwritten by an older read that was
// still in flight when the broadcast arrived.

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  keyboard: vi.fn<() => Promise<KeyboardConfig | null>>(),
}));

vi.mock("#lib/data/demo-items.ts", async (original) => {
  const source = await original<typeof import("#lib/data/demo-items.ts")>();
  return {
    ...source,
    demoClipboardItems: source.demoClipboardItems.map((item, index) => ({
      ...item,
      id: index === 0 ? 'import-"quoted' : index === 1 ? "import-\\slash" : item.id,
    })),
  };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name: string, callback: (event: { payload: unknown }) => void) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));
vi.mock("#lib/services/keyboard.ts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/services/keyboard.ts")>()),
  getKeyboardConfig: () => bridge.keyboard(),
}));

let app: ReturnType<typeof mount> | undefined;
let target: HTMLDivElement;

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

async function open() {
  target = document.createElement("div");
  document.body.append(target);
  app = mount(MainPage, { target });
  flushSync();
  await settle();
}

const emit = (name: string, payload: unknown) => bridge.listeners.get(name)?.({ payload });
const firstTabTitle = () => target.querySelector<HTMLElement>('[role="tab"]')!.title;

beforeEach(() => {
  bridge.listeners.clear();
  bridge.keyboard.mockReset().mockResolvedValue({ shortcuts: {} });
  vi.spyOn(HTMLCanvasElement.prototype, "getContext").mockReturnValue({
    measureText: (text: string) => ({ width: text.length * 7 }),
  } as unknown as CanvasRenderingContext2D);
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
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("main window keyboard config sync", () => {
  it("applies a broadcast config to the toolbar tooltip without a focus reload", async () => {
    await open();
    expect(firstTabTitle()).toMatch(/\(Alt\+1\)$/);

    emit("keyboard-config-changed", {
      shortcuts: { switchFilter1: ["Ctrl+Shift+1", "Alt+9"] },
    });
    await settle();

    // Only the first binding of a multi-chord action is shown.
    expect(firstTabTitle()).toMatch(/\(Ctrl\+Shift\+1\)$/);
    expect(firstTabTitle()).not.toContain("Alt+9");
  });

  it("removes the hint when the broadcast disables the action", async () => {
    await open();
    emit("keyboard-config-changed", { shortcuts: { switchFilter1: [] } });
    await settle();
    expect(firstTabTitle()).not.toContain("(");
  });

  it("keeps the broadcast config when an older read resolves afterwards", async () => {
    let resolveStale!: (config: KeyboardConfig) => void;
    bridge.keyboard.mockReturnValueOnce(
      new Promise<KeyboardConfig>((resolve) => {
        resolveStale = resolve;
      }),
    );
    await open();

    emit("keyboard-config-changed", { shortcuts: { switchFilter1: ["Ctrl+Shift+1"] } });
    await settle();
    expect(firstTabTitle()).toMatch(/\(Ctrl\+Shift\+1\)$/);

    // The mount-time read finishes late with the previous snapshot.
    resolveStale({ shortcuts: { switchFilter1: ["Alt+1"] } });
    await settle();
    expect(firstTabTitle()).toMatch(/\(Ctrl\+Shift\+1\)$/);
  });

  it("unregisters the listener on unmount", async () => {
    await open();
    expect(bridge.listeners.has("keyboard-config-changed")).toBe(true);
    await unmount(app!);
    app = undefined;
    expect(bridge.listeners.has("keyboard-config-changed")).toBe(false);
  });
});

it("moves and focuses the real route across IDs containing CSS delimiters", async () => {
  await open();
  const cards = [...target.querySelectorAll<HTMLElement>("[data-id]")];
  expect(cards.length).toBeGreaterThan(1);
  const first = cards[0];
  const second = cards[1];
  first.scrollIntoView = vi.fn();
  second.scrollIntoView = vi.fn();
  first
    .querySelector<HTMLElement>(".card-select")!
    .dispatchEvent(new MouseEvent("click", { bubbles: true }));
  flushSync();
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key: "ArrowDown", bubbles: true, cancelable: true }),
  );
  await settle();
  expect(second.scrollIntoView).toHaveBeenCalledWith({ block: "nearest" });
  expect(document.activeElement).toBe(second);
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key: "ArrowUp", bubbles: true, cancelable: true }),
  );
  await settle();
  expect(document.activeElement).toBe(first);
});
