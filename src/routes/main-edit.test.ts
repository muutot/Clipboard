import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { t } from "$lib/i18n";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";

// The browser (non-Tauri) runtime seeds `demoClipboardItems`, so the mounted
// main route renders real rows without a backend. The inline-edit draft is
// owned by the route, so it must outlive the card unmounting when a filter
// change (or virtual scrolling) removes the row from the list.

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  hydrate: vi.fn(),
  invoke: vi.fn(async () => true),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name, callback) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  hydrateClipboardItem: bridge.hydrate,
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.listeners.clear();
  bridge.hydrate.mockReset();
  bridge.invoke.mockReset().mockResolvedValue(true);
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
  it("does not close a newer editor when an earlier media save completes", async () => {
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
    const media = {
      id: "saving-media",
      kind: "image",
      title: "old-image",
      contentHash: "image-hash",
      sourceApp: "test",
      sizeBytes: 20,
      createdAtMs: 1,
      lastUsedAtMs: 1,
      isFavorite: false,
    } as PersistedClipboardItem;
    const text = {
      ...media,
      id: "newer-text",
      kind: "text",
      title: "newer-text",
      textContent: "BODY",
    } as PersistedClipboardItem;
    for (const record of [media, text])
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
    flushSync();
    const edit = async (id: string, label: string) => {
      target
        .querySelector<HTMLElement>(`[data-id="${id}"]`)!
        .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }));
      flushSync();
      buttonByLabel(target, ".context-menu button", label).click();
      await settle();
    };
    await edit(media.id, t("edit.editFileName"));
    const renameInput = target.querySelector<HTMLTextAreaElement>(
      `[data-id="${media.id}"] textarea`,
    )!;
    renameInput.value = "renamed-image";
    renameInput.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    let finish!: (value: unknown) => void;
    bridge.invoke.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    target.querySelector<HTMLButtonElement>(`[data-id="${media.id}"] .edit-save`)!.click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith("rename_item", {
      id: media.id,
      newName: "renamed-image",
    });
    await edit(text.id, t("card.edit"));
    const draft = target.querySelector<HTMLTextAreaElement>(`[data-id="${text.id}"] textarea`)!;
    draft.value = "PRESERVE NEW DRAFT";
    draft.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    finish({ ...media, title: "renamed-image" });
    await settle();
    expect(
      target.querySelector<HTMLTextAreaElement>(`[data-id="${text.id}"] textarea`)?.value,
    ).toBe("PRESERVE NEW DRAFT");
  });
  it("keeps the newer editor and draft when an earlier hydration finishes later", async () => {
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
    const pending = new Map<
      string,
      { item: ClipboardItem; finish: (item: ClipboardItem) => void }
    >();
    bridge.hydrate.mockImplementation(
      (item: ClipboardItem) =>
        new Promise<ClipboardItem>((finish) => pending.set(item.id, { item, finish })),
    );
    for (const id of ["slow-editor", "new-editor"]) {
      const record = {
        id,
        kind: "text",
        title: id,
        textContent: "preview",
        contentHash: id,
        contentLoaded: false,
        sourceApp: "test",
        sizeBytes: 20,
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      const card = target.querySelector<HTMLElement>(`[data-id="${id}"]`)!;
      card.dispatchEvent(
        new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }),
      );
      flushSync();
      buttonByLabel(target, ".context-menu button", t("card.edit")).click();
      await settle();
    }
    const newer = pending.get("new-editor")!;
    newer.finish({ ...newer.item, contentLoaded: true, textContent: "NEWER FULL BODY" });
    await settle();
    const draft = target.querySelector<HTMLTextAreaElement>('[data-id="new-editor"] textarea');
    expect(draft).not.toBeNull();
    draft!.value = "UNSAVED NEWER DRAFT";
    draft!.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const older = pending.get("slow-editor")!;
    older.finish({ ...older.item, contentLoaded: true, textContent: "OLDER FULL BODY" });
    await settle();
    expect(target.querySelector('[data-id="slow-editor"] textarea')).toBeNull();
    expect(
      target.querySelector<HTMLTextAreaElement>('[data-id="new-editor"] textarea')?.value,
    ).toBe("UNSAVED NEWER DRAFT");
  });
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
