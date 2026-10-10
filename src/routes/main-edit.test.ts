import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { t } from "$lib/i18n";

// The browser (non-Tauri) runtime seeds `demoClipboardItems`, so the mounted
// main route renders real rows without a backend. The inline-edit draft is
// owned by the route, so it must outlive the card unmounting when a filter
// change (or virtual scrolling) removes the row from the list.

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.unstubAllGlobals();
});

async function settle(rounds = 16) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
  }
}

function buttonByLabel(root: ParentNode, scope: string, label: string): HTMLButtonElement {
  const button = [...root.querySelectorAll<HTMLButtonElement>(scope)].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (!button) throw new Error(`button not found: ${label}`);
  return button;
}

describe("main window inline editing", () => {
  it("keeps the draft when a filter change unmounts the edited card", async () => {
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

    const card = target.querySelector<HTMLElement>('[data-id="demo-text-clipboard"]');
    expect(card).not.toBeNull();
    card!.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }));
    flushSync();

    buttonByLabel(target, ".context-menu button", t("card.edit")).click();
    await settle();

    const textarea = target.querySelector<HTMLTextAreaElement>(
      '[data-id="demo-text-clipboard"] textarea',
    );
    expect(textarea).not.toBeNull();
    textarea!.value = "DRAFT EDIT CONTENT";
    textarea!.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();

    // Switch to the link filter, which removes this text row (and its card).
    buttonByLabel(target, '.filters [role="tab"]', t("filter.link")).click();
    await settle();
    expect(target.querySelector('[data-id="demo-text-clipboard"]')).toBeNull();

    // Returning must restore the editor with the uncommitted draft intact.
    buttonByLabel(target, '.filters [role="tab"]', t("filter.all")).click();
    await settle();

    const reopened = target.querySelector<HTMLTextAreaElement>(
      '[data-id="demo-text-clipboard"] textarea',
    );
    expect(reopened).not.toBeNull();
    expect(reopened!.value).toBe("DRAFT EDIT CONTENT");
  });
});
