import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";
import { t } from "$lib/i18n";
import { toClipboardItem } from "$lib/services/clipboard/mapping";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  hydrate: vi.fn(),
  materialize: vi.fn(),
}));
vi.mock("$lib/services/clipboard", async (importOriginal) => ({
  ...(await importOriginal<typeof import("$lib/services/clipboard")>()),
  hydrateClipboardItem: bridge.hydrate,
  materializeClipboardItem: bridge.materialize,
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
  it.each(["image", "file"] as const)(
    "keeps a recycled %s in the recycle bin after opening its materialized detail",
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
        id: `recycled-${kind}`,
        kind,
        title: "Recycled media",
        sourceApp: "test",
        sizeBytes: 10,
        contentHash: "media-hash",
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      bridge.listeners.get("clipboard-items-changed")?.({ payload: { deletedIds: [record.id] } });
      flushSync();
      const selectFilter = (key: string) => {
        const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
          (button) => button.textContent?.trim() === t(key),
        );
        tab!.click();
        flushSync();
      };
      selectFilter("filter.deleted");
      expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
      // The materialization command returns the regular persisted DTO, which
      // omits recycle-bin membership even for an already deleted record.
      bridge.materialize.mockResolvedValue(
        toClipboardItem({ ...record, resourcePath: `C:/fixture/${kind}.png` }),
      );
      target
        .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
        .dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
      for (let i = 0; i < 16; i++) {
        await Promise.resolve();
        flushSync();
      }
      expect(bridge.materialize).toHaveBeenCalledWith(expect.objectContaining({ deleted: true }));
      expect(target.querySelector(".detail-panel")?.textContent).toContain("Recycled media");
      expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
      selectFilter("filter.all");
      expect(target.querySelector(`[data-id="${record.id}"]`)).toBeNull();
    },
  );
  it("keeps a recycled row visible after its saved tags arrive", async () => {
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
      id: "recycled-tags",
      kind: "text",
      title: "Recycled content",
      textContent: "Recycled content",
      sourceApp: "test",
      sizeBytes: 16,
      contentHash: "recycled-hash",
      createdAtMs: 1,
      lastUsedAtMs: 1,
      isFavorite: false,
    } as PersistedClipboardItem;
    bridge.listeners.get("clipboard-item-added")?.({ payload: record });
    bridge.listeners.get("clipboard-items-changed")?.({ payload: { deletedIds: [record.id] } });
    flushSync();
    const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
      (button) => button.textContent?.trim() === t("filter.deleted"),
    );
    tab!.click();
    flushSync();
    expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
    bridge.listeners.get("clipboard-items-changed")?.({
      payload: {
        items: [{ ...record, metadataJson: JSON.stringify({ tags: ["saved-bin-tag"] }) }],
      },
    });
    for (let i = 0; i < 12; i++) {
      await Promise.resolve();
      flushSync();
    }
    expect(target.querySelector(`[data-id="${record.id}"]`)?.textContent).toContain(
      "saved-bin-tag",
    );
    const active = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
      (button) => button.textContent?.trim() === t("filter.all"),
    );
    active!.click();
    flushSync();
    expect(target.querySelector(`[data-id="${record.id}"]`)).toBeNull();
  });
  it.each(["rename", "favorite"])(
    "preserves a later %s while media materialization is in flight",
    async (change) => {
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
        id: "pending-media",
        kind: "image",
        title: "Original image",
        contentHash: "image-hash",
        sizeBytes: 10,
        sourceApp: "test",
        createdAtMs: 1,
        lastUsedAtMs: 1,
        isFavorite: false,
      } as PersistedClipboardItem;
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      let finish!: (item: ClipboardItem) => void;
      bridge.materialize.mockImplementation(
        () => new Promise<ClipboardItem>((resolve) => (finish = resolve)),
      );
      bridge.listeners.get("clipboard-open-detail")?.({ payload: toClipboardItem(record) });
      for (let i = 0; i < 8; i++) {
        await Promise.resolve();
        flushSync();
      }
      expect(bridge.materialize).toHaveBeenCalled();
      const updated = { ...record, title: "Latest image", isFavorite: true };
      bridge.listeners.get("clipboard-items-changed")?.({ payload: { items: [updated] } });
      flushSync();
      finish({ ...toClipboardItem(record), resourcePath: "C:/fixture/image.png" });
      for (let i = 0; i < 16; i++) {
        await Promise.resolve();
        flushSync();
      }
      if (change === "favorite") {
        const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
          (button) => button.textContent?.trim() === t("filter.favorite"),
        );
        tab!.click();
        flushSync();
        expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
      }
      expect(target.querySelector(".detail-panel")?.textContent).toContain("Latest image");
    },
  );
  it.each([
    "edit",
    "outside-edit",
    "favorite",
    "remove",
    "outside-remove",
    "soft-delete",
    "outside-soft-delete",
  ])("does not overwrite a later %s event with a delayed detail response", async (change) => {
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
      id: "delayed-detail",
      kind: "text",
      title: "Original detail title",
      textContent: "OLD BODY",
      contentHash: "old-hash",
      contentLoaded: false,
      sourceApp: "test",
      sizeBytes: 8,
      createdAtMs: 1,
      lastUsedAtMs: 1,
      isFavorite: false,
    } as PersistedClipboardItem;
    const summary = {
      id: record.id,
      kind: "text",
      title: record.title,
      textContent: "preview",
      contentHash: record.contentHash,
      contentLoaded: false,
      preview: "preview",
      sourceApp: "test",
      sourceTone: "neutral",
      sizeLabel: "8 chars",
      createdAt: 1,
      favorite: false,
    } as ClipboardItem;
    if (!change.startsWith("outside-")) {
      bridge.listeners.get("clipboard-item-added")?.({ payload: record });
      flushSync();
      expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
    }
    let finish!: (item: ClipboardItem) => void;
    bridge.hydrate.mockImplementation(
      () => new Promise<ClipboardItem>((resolve) => (finish = resolve)),
    );
    bridge.listeners.get("clipboard-open-detail")?.({ payload: summary });
    flushSync();
    const removed = change.endsWith("remove") || change.endsWith("soft-delete");
    bridge.listeners.get("clipboard-items-changed")?.({
      payload: removed
        ? change.endsWith("soft-delete")
          ? { deletedIds: [record.id] }
          : { removedIds: [record.id] }
        : {
            items: [
              {
                ...record,
                contentLoaded: change.endsWith("edit"),
                ...(change.endsWith("edit")
                  ? { title: "LATEST TITLE", textContent: "LATEST BODY", contentHash: "new-hash" }
                  : { isFavorite: true }),
              },
            ],
          },
    });
    flushSync();
    if (change === "favorite") {
      const favoriteTab = [
        ...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]'),
      ].find((button) => button.textContent?.trim() === t("filter.favorite"));
      expect(favoriteTab).toBeDefined();
      favoriteTab!.click();
      flushSync();
      expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
    }
    finish({ ...summary, textContent: "OLD BODY", contentLoaded: true });
    for (let i = 0; i < 16; i++) {
      await Promise.resolve();
      flushSync();
    }
    if (removed) {
      expect(target.querySelector(".detail-panel")).toBeNull();
      expect(target.querySelector(`[data-id="${record.id}"]`)).toBeNull();
    } else if (change.endsWith("edit")) {
      if (change === "edit")
        expect(target.querySelector(`[data-id="${record.id}"]`)?.textContent).toContain(
          "LATEST BODY",
        );
      expect(target.querySelector(".detail-panel")?.textContent).toContain("LATEST BODY");
      expect(target.querySelector(".detail-panel")?.textContent).not.toContain("OLD BODY");
    } else {
      expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
    }
  });
  it("loads a summary before showing its complete detail", async () => {
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
    const summary = {
      id: "summary",
      kind: "text",
      title: "Large record",
      textContent: "preview",
      contentLoaded: false,
      preview: "",
      sourceApp: "test",
      sourceTone: "neutral",
      sizeLabel: "4 KB",
      createdAt: 0,
      favorite: false,
    } as ClipboardItem;
    let finish!: (item: ClipboardItem) => void;
    bridge.hydrate.mockImplementation(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    bridge.listeners.get("clipboard-open-detail")?.({ payload: summary });
    flushSync();
    expect(target.querySelector(".detail-panel")).toBeNull();
    finish({ ...summary, textContent: "FULL CONTENT", contentLoaded: true });
    for (let i = 0; i < 12; i++) {
      await Promise.resolve();
      flushSync();
    }
    expect(target.querySelector(".detail-panel")?.textContent).toContain("FULL CONTENT");
  });
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
