import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";

// The browser (non-Tauri) runtime seeds `demoClipboardItems`, so the mounted
// main route renders real rows without a backend. A shift-click range doubles
// as the paste queue, and each selected card shows its 1-based position badge
// (data-testid="card-order"). The range must be numbered in the direction the
// user selected: starting low and shift-clicking upward reads 6-1 down the
// list, not 1-6.

vi.mock("$lib/services/window-control", () => ({
  hideMainWindow: vi.fn(async () => {}),
  showMainWindow: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.unstubAllGlobals();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

async function setup() {
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
  return target;
}

/** Card ids in the same order `filteredItems` renders them (no virtual scroll for the demo set). */
function cardIds(root: ParentNode): string[] {
  return [...root.querySelectorAll<HTMLElement>(".clip-card[data-id]")].map(
    (card) => card.dataset.id!,
  );
}

function clickCard(root: ParentNode, id: string, init: MouseEventInit = {}) {
  const card = root.querySelector<HTMLElement>(`[data-id="${id}"]`);
  if (!card) throw new Error(`card not found: ${id}`);
  card
    .querySelector<HTMLElement>(".card-select")!
    .dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, ...init }));
  flushSync();
}

/** id -> rendered 1-based paste position, read from the visible order badges. */
function orderBadges(root: ParentNode): Map<string, number> {
  const badges = new Map<string, number>();
  for (const card of root.querySelectorAll<HTMLElement>(".clip-card[data-id]")) {
    const badge = card.querySelector<HTMLElement>('[data-testid="card-order"]');
    if (badge) badges.set(card.dataset.id!, Number(badge.textContent?.trim()));
  }
  return badges;
}

describe("main window shift-click range order", () => {
  it("numbers a top-to-bottom range 1..N down the list", async () => {
    const target = await setup();
    const ids = cardIds(target);
    expect(ids.length).toBeGreaterThanOrEqual(7);

    clickCard(target, ids[1]); // plain click sets the anchor
    clickCard(target, ids[6], { shiftKey: true }); // extend downward

    const badges = orderBadges(target);
    expect(badges.size).toBe(6);
    for (let i = 1; i <= 6; i++) expect(badges.get(ids[i])).toBe(i);
  });

  it("numbers a bottom-to-top range 6..1 down the list", async () => {
    const target = await setup();
    const ids = cardIds(target);
    expect(ids.length).toBeGreaterThanOrEqual(7);

    clickCard(target, ids[6]); // anchor low in the list
    clickCard(target, ids[1], { shiftKey: true }); // extend upward

    const badges = orderBadges(target);
    expect(badges.size).toBe(6);
    // The anchor (bottom) is #1; the sequence reads 6,5,4,3,2,1 top-to-bottom.
    for (let i = 1; i <= 6; i++) expect(badges.get(ids[i])).toBe(7 - i);
  });
});
