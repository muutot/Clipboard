import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";

// The browser (non-Tauri) runtime seeds `demoClipboardItems`, so the mounted
// main route renders real rows without a backend. The main-window hide call
// goes through `@tauri-apps/api/window`, mocked here so the test can prove
// that an Escape press over a bulk selection exits the selection instead of
// hiding the window (hideWindow's default chord is Escape).

const bridge = vi.hoisted(() => ({
  hide: vi.fn(async () => {}),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ hide: bridge.hide }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.hide.mockClear();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

/** Ctrl+click a card's select surface, which is how multi-select is entered. */
function ctrlClickCard(root: ParentNode, id: string) {
  const card = root.querySelector<HTMLElement>(`[data-id="${id}"]`);
  if (!card) throw new Error(`card not found: ${id}`);
  card
    .querySelector<HTMLElement>(".card-select")!
    .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true }));
  flushSync();
}

function pressEscape() {
  window.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
  );
  flushSync();
}

describe("main window Escape over a bulk selection", () => {
  it("exits multi-select instead of hiding the window", async () => {
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
    // Let the startup loads settle while the runtime is still non-Tauri, so
    // the demo rows stay mounted when the stub is installed below.
    await settle();

    ctrlClickCard(target, "demo-text-clipboard");
    ctrlClickCard(target, "demo-text-cutout");
    expect(target.querySelector(".bulk-bar")?.textContent).toContain("2");

    // From this point the route sees the Tauri runtime, so an unguarded
    // Escape would hide the main window through the default hideWindow chord.
    Object.defineProperty(window, "__TAURI_INTERNALS__", {
      configurable: true,
      value: { invoke: vi.fn(async () => []) },
    });

    pressEscape();
    expect(target.querySelector(".bulk-bar")).toBeNull();
    expect(bridge.hide).not.toHaveBeenCalled();

    // The selection is gone, so a second press still hides the window.
    pressEscape();
    expect(bridge.hide).toHaveBeenCalledTimes(1);
  });
});
