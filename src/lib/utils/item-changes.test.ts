import { describe, expect, it } from "vitest";
import type {
  ClipboardItem,
  ClipboardItemsChangedPayload,
  PersistedClipboardItem,
} from "$lib/types/clipboard";
import { appendItems, createItemStore, getDetailItem, getItems, setDetailItem } from "./item-store";
import { applyItemsChangedEvent } from "./item-changes";

function item(id: string, overrides: Partial<ClipboardItem> = {}): ClipboardItem {
  return {
    id,
    kind: "text",
    title: `title-${id}`,
    preview: `text-${id}`,
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "1 B",
    createdAt: 0,
    favorite: false,
    deleted: false,
    tags: [],
    ...overrides,
  };
}

/** The persisted shape the backend broadcasts, kept minimal on purpose. */
function persisted(id: string, overrides: Partial<PersistedClipboardItem> = {}) {
  return {
    id,
    kind: "text",
    title: `title-${id}`,
    textContent: `text-${id}`,
    contentHash: `hash-${id}`,
    sourceApp: "test",
    sizeBytes: 1,
    createdAtMs: 0,
    lastUsedAtMs: null,
    isFavorite: false,
    ...overrides,
  } as unknown as PersistedClipboardItem;
}

/** One record in the history and indexed views, one in the spare cache. */
function shared() {
  let store = createItemStore([item("a"), item("b")]);
  store = appendItems(store, [item("a")], "indexed");
  store = appendItems(store, [item("a")], "cache");
  return store;
}

describe("applyItemsChangedEvent", () => {
  it("replaces a record in every view that displays it", () => {
    // A favorite toggled in the float panel arrives here: one payload, every
    // view of the main window updates together.
    const next = applyItemsChangedEvent(shared(), {
      items: [persisted("a", { isFavorite: true })],
      deletedIds: [],
      restoredIds: [],
      removedIds: [],
    });
    expect(getItems(next, "history")[0].favorite).toBe(true);
    expect(getItems(next, "indexed")![0].favorite).toBe(true);
    expect(getItems(next, "cache")[0].favorite).toBe(true);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("ignores records this window never loaded", () => {
    const next = applyItemsChangedEvent(shared(), {
      items: [persisted("not-loaded", { isFavorite: true })],
      deletedIds: [],
      restoredIds: [],
      removedIds: [],
    });
    // Writing it in would resurrect a row the user cannot see.
    expect(next.byId.has("not-loaded")).toBe(false);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("drops permanently deleted ids from every view, cache included", () => {
    const next = applyItemsChangedEvent(shared(), {
      items: [],
      deletedIds: [],
      restoredIds: [],
      removedIds: ["a"],
    });
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["b"]);
    expect(getItems(next, "indexed")).toEqual([]);
    expect(getItems(next, "cache")).toEqual([]);
    expect(next.byId.has("a")).toBe(false);
  });

  it("closes the detail pane when its record was deleted", () => {
    const opened = setDetailItem(shared(), item("a"));
    expect(getDetailItem(opened)?.id).toBe("a");
    const next = applyItemsChangedEvent(opened, {
      items: [],
      deletedIds: [],
      restoredIds: [],
      removedIds: ["a"],
    });
    expect(getDetailItem(next)).toBeNull();
  });

  it("marks a soft delete as deleted without dropping the row", () => {
    // The deleted flag is receiver state, not a stored column, so it travels as
    // an id. The row must stay in the loaded list for the recycle-bin filter.
    const next = applyItemsChangedEvent(shared(), {
      items: [],
      deletedIds: ["a"],
      restoredIds: [],
      removedIds: [],
    });
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b"]);
    expect(getItems(next, "history")[0].deleted).toBe(true);
    // The flag is the one record, so every view agrees.
    expect(getItems(next, "indexed")![0].deleted).toBe(true);
    expect(getDetailItem(setDetailItem(shared(), item("a")))).not.toBeNull();
  });

  it("clears the deleted flag on restore", () => {
    const deleted = applyItemsChangedEvent(shared(), {
      items: [],
      deletedIds: ["a"],
      restoredIds: [],
      removedIds: [],
    });
    const next = applyItemsChangedEvent(deleted, {
      items: [],
      deletedIds: [],
      restoredIds: ["a"],
      removedIds: [],
    });
    expect(getItems(next, "history")[0].deleted).toBe(false);
  });

  it("lets a removal win over a patch in the same payload", () => {
    // An id that is gone must not survive a later content update.
    const next = applyItemsChangedEvent(shared(), {
      items: [persisted("a", { isFavorite: true }), persisted("b", { title: "edited" })],
      deletedIds: [],
      restoredIds: [],
      removedIds: ["a"],
    });
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["b"]);
    expect(getItems(next, "history")[0].title).toBe("edited");
    expect(next.byId.has("a")).toBe(false);
  });

  it("tolerates a payload with the lists omitted", () => {
    const next = applyItemsChangedEvent(shared(), {} as ClipboardItemsChangedPayload);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b"]);
  });
});
