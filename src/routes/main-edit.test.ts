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
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
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

describe("main window optimistic favorite rollback", () => {
  it.each(["title", "tags"] as const)(
    "returns to confirmed %s after consecutive save failures",
    async (field) => {
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
      const record = {
        id: `failed-${field}`,
        kind: "text",
        title: "CONFIRMED TITLE",
        textContent: "body",
        contentHash: `failed-${field}`,
        sourceApp: "test",
        sizeBytes: 4,
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
        metadataJson: JSON.stringify({ tags: ["confirmed-tag"] }),
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      // Open a full browser fixture first, then enable only metadata IPC.
      target
        .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
        .dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
      await settle();
      expect(target.querySelector(".detail-panel")).not.toBeNull();
      const failures: ((reason: Error) => void)[] = [];
      const first = new Promise<boolean>((_, reject) => failures.push(reject));
      const second = new Promise<boolean>((_, reject) => failures.push(reject));
      void first.catch(() => {});
      void second.catch(() => {});
      const command = field === "title" ? "rename_item" : "set_clipboard_item_tags";
      let calls = 0;
      Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
      bridge.invoke.mockImplementation((name: string) => {
        if (name === command) return calls++ === 0 ? first : second;
        return new Promise(() => {});
      });
      if (field === "tags") {
        buttonByLabel(target, ".detail-tabs button", t("detail.tags")).click();
        flushSync();
      }
      const save = async (value: string) => {
        if (field === "title") {
          target
            .querySelector<HTMLElement>(".header-title")!
            .dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
          flushSync();
          const input = target.querySelector<HTMLInputElement>(".header-title-input")!;
          input.value = value;
          input.dispatchEvent(new Event("input", { bubbles: true }));
          input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        } else {
          const input = target.querySelector<HTMLInputElement>(".tags-tab input")!;
          input.value = value;
          input.dispatchEvent(new Event("input", { bubbles: true }));
          flushSync();
          input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
        }
        await settle();
      };
      await save("first-draft");
      await save("second-draft");
      failures[0](new Error("first failed"));
      await settle();
      failures[1](new Error("second failed"));
      await settle();
      if (field === "title") {
        expect(target.querySelector(".header-title")?.textContent).toBe("CONFIRMED TITLE");
      } else {
        expect(target.querySelector(".tags-list")?.textContent).toContain("confirmed-tag");
        expect(target.querySelector(".tags-list")?.textContent).not.toContain("first-draft");
        expect(target.querySelector(".tags-list")?.textContent).not.toContain("second-draft");
      }
    },
  );

  it.each(["first-then-second", "second-then-first"])(
    "restores the confirmed favorite after both requests fail (%s)",
    async (order) => {
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
      const record = {
        id: "failed-favorites",
        kind: "text",
        title: "failed-favorites",
        textContent: "body",
        contentHash: "favorite-body",
        sourceApp: "test",
        sizeBytes: 4,
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
      let failFirst!: (reason: Error) => void;
      let failSecond!: (reason: Error) => void;
      const first = new Promise<boolean>((_, reject) => (failFirst = reject));
      const second = new Promise<boolean>((_, reject) => (failSecond = reject));
      // A queued request may not have reached IPC when its prepared mock
      // failure settles. Keep that mock promise handled until it is consumed.
      void first.catch(() => {});
      void second.catch(() => {});
      // Keep revalidation reads pending; this test exercises the actual route,
      // services and IPC rejection handling without a native backend.
      let favoriteCalls = 0;
      bridge.invoke.mockImplementation((command: string) => {
        if (command === "set_clipboard_item_favorite")
          return favoriteCalls++ === 0 ? first : second;
        return new Promise(() => {});
      });
      const toggle = (currentlyFavorite: boolean) => {
        target
          .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
          .dispatchEvent(
            new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }),
          );
        flushSync();
        buttonByLabel(
          target,
          ".context-menu button",
          t(currentlyFavorite ? "card.unfavorite" : "card.favorite"),
        ).click();
        flushSync();
      };
      toggle(false);
      await settle();
      toggle(true);
      await settle();
      if (order === "first-then-second") {
        failFirst(new Error("synthetic first failure"));
        await settle();
        failSecond(new Error("synthetic second failure"));
      } else {
        failSecond(new Error("synthetic second failure"));
        await settle();
        failFirst(new Error("synthetic first failure"));
      }
      await settle();
      target
        .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
        .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }));
      flushSync();
      expect(
        [...target.querySelectorAll<HTMLButtonElement>(".context-menu button")].some(
          (button) => button.textContent?.trim() === t("card.favorite"),
        ),
      ).toBe(true);
    },
  );
});

describe("main window inline editing", () => {
  it.each(["text", "image", "file"] as const)(
    "does not overwrite a later %s edit event when an earlier save returns",
    async (kind) => {
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
      const record = {
        id: `delayed-save-${kind}`,
        kind,
        title: "Initial record",
        textContent: kind === "text" ? "INITIAL BODY" : null,
        contentHash: "initial-hash",
        resourcePath: kind === "text" ? null : "C:/fixture/initial.png",
        sourceApp: "test",
        sizeBytes: 20,
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      target
        .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
        .dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, clientX: 12, clientY: 12 }));
      flushSync();
      buttonByLabel(
        target,
        ".context-menu button",
        t(kind === "text" ? "card.edit" : "edit.editFileName"),
      ).click();
      await settle();
      const draft = target.querySelector<HTMLTextAreaElement>(`[data-id="${record.id}"] textarea`)!;
      draft.value = "OLDER SAVE";
      draft.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      // Enable only the save's IPC branch after mounting the browser fixture;
      // the native startup/list/clipboard boundaries are never exercised.
      Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
      let finish!: (value: unknown) => void;
      bridge.invoke.mockImplementation((command: string) =>
        command === (kind === "text" ? "update_clipboard_text" : "rename_item")
          ? new Promise((resolve) => (finish = resolve))
          : Promise.resolve(true),
      );
      target.querySelector<HTMLButtonElement>(`[data-id="${record.id}"] .edit-save`)!.click();
      await settle();
      expect(bridge.invoke).toHaveBeenCalledWith(
        kind === "text" ? "update_clipboard_text" : "rename_item",
        expect.objectContaining({ id: record.id }),
      );
      // The original command has committed; a later edit in another window has
      // also committed and broadcast its newer record before the old IPC reply.
      bridge.listeners.get("clipboard-items-changed")?.({
        payload: {
          items: [
            {
              ...record,
              title: "LATEST COMMITTED TITLE",
              textContent: kind === "text" ? "LATEST COMMITTED BODY" : null,
              contentHash: "latest-hash",
              resourcePath: kind === "text" ? null : "C:/fixture/latest.png",
            },
          ],
        },
      });
      flushSync();
      finish(
        kind === "text"
          ? true
          : { ...record, title: "OLDER SAVE", resourcePath: "C:/fixture/older.png" },
      );
      await settle();
      const card = target.querySelector(`[data-id="${record.id}"]`)!;
      expect(card.querySelector("textarea")).toBeNull();
      if (kind === "text") {
        expect(card.textContent).toContain("LATEST COMMITTED TITLE");
        expect(card.textContent).toContain("LATEST COMMITTED BODY");
      } else {
        Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
        card.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
        await settle();
        expect(target.querySelector(".detail-panel")?.textContent).toContain(
          "LATEST COMMITTED TITLE",
        );
        if (kind === "image") {
          buttonByLabel(target, ".detail-panel button", t("detail.details")).click();
          flushSync();
          expect(target.querySelector(".detail-panel")?.textContent).toContain(
            "C:/fixture/latest.png",
          );
        } else expect(card.textContent).toContain("latest.png");
      }
      expect(card.textContent).not.toContain("OLDER SAVE");
    },
  );
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
