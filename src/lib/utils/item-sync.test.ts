import { describe, expect, it } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import {
  applyItemPatchesToCopies,
  findLoadedItemInCopies,
  mergeDeletedHistoryPage,
  removeItemTag,
  removeItemsFromCopies,
  replaceItemInCopies,
  rewriteItemTags,
  type ItemCopies,
} from "./item-sync";

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
    tags: [],
    ...overrides,
  };
}

/** One id present in all four copies with per-copy divergence. */
function copies(): ItemCopies {
  return {
    items: [item("a"), item("b")],
    indexedItems: [item("a", { title: "stale-a" }), item("b")],
    searchCache: [item("a")],
    detailItem: item("a", { favorite: true }),
  };
}

describe("applyItemPatchesToCopies", () => {
  it("fans a patch out to every copy, including searchCache", () => {
    // Regression: a tags update once skipped searchCache, so search results
    // showed stale tags while the list showed fresh ones.
    const next = applyItemPatchesToCopies(copies(), new Map([["a", { tags: ["work"] }]]));
    expect(next.items.find((i) => i.id === "a")?.tags).toEqual(["work"]);
    expect(next.indexedItems?.find((i) => i.id === "a")?.tags).toEqual(["work"]);
    expect(next.searchCache.find((i) => i.id === "a")?.tags).toEqual(["work"]);
    expect(next.detailItem?.tags).toEqual(["work"]);
  });

  it("leaves untouched entries and missing copies alone", () => {
    const before = copies();
    const next = applyItemPatchesToCopies(
      { ...before, indexedItems: null, detailItem: null },
      new Map([["a", { favorite: true }]]),
    );
    expect(next.items.find((i) => i.id === "b")).toBe(before.items[1]);
    expect(next.indexedItems).toBeNull();
    expect(next.detailItem).toBeNull();
    expect(next.searchCache).toHaveLength(1);
  });

  it("is a no-op for empty patch sets", () => {
    const before = copies();
    expect(applyItemPatchesToCopies(before, new Map())).toBe(before);
  });
});

describe("removeItemsFromCopies", () => {
  it("purges ids from every copy and nulls a removed detail item", () => {
    const next = removeItemsFromCopies(copies(), new Set(["a"]));
    expect(next.items.map((i) => i.id)).toEqual(["b"]);
    expect(next.indexedItems?.map((i) => i.id)).toEqual(["b"]);
    expect(next.searchCache).toHaveLength(0);
    expect(next.detailItem).toBeNull();
  });

  it("keeps an unrelated detail item and is a no-op for empty sets", () => {
    const before = { ...copies(), detailItem: item("z") };
    const next = removeItemsFromCopies(before, new Set(["a"]));
    expect(next.detailItem?.id).toBe("z");
    expect(removeItemsFromCopies(before, new Set())).toBe(before);
  });
});

describe("replaceItemInCopies", () => {
  it("swaps the whole record wherever its id appears", () => {
    const updated = item("a", { title: "materialized" });
    const next = replaceItemInCopies(copies(), updated);
    expect(next.items[0]).toBe(updated);
    expect(next.indexedItems?.[0]).toBe(updated);
    expect(next.searchCache[0]).toBe(updated);
    expect(next.detailItem).toBe(updated);
    expect(next.items[1]).not.toBe(updated);
  });
});

describe("findLoadedItemInCopies", () => {
  it("follows display-priority order and returns undefined when absent", () => {
    const state = copies();
    expect(findLoadedItemInCopies(state, "b")?.title).toBe("title-b");
    expect(findLoadedItemInCopies(state, "a")?.title).toBe("title-a");
    const cacheOnly: ItemCopies = {
      items: [],
      indexedItems: null,
      searchCache: [item("c")],
      detailItem: null,
    };
    expect(findLoadedItemInCopies(cacheOnly, "c")?.id).toBe("c");
    const detailOnly: ItemCopies = {
      items: [],
      indexedItems: null,
      searchCache: [],
      detailItem: item("d"),
    };
    expect(findLoadedItemInCopies(detailOnly, "d")?.id).toBe("d");
    expect(findLoadedItemInCopies(state, "missing")).toBeUndefined();
  });
});

describe("rewriteItemTags", () => {
  it("renames the tag and drops duplicates the rename would create", () => {
    expect(rewriteItemTags(item("a", { tags: ["x", "y"] }), "x", "z").tags).toEqual(["z", "y"]);
    expect(rewriteItemTags(item("a", { tags: ["x", "z"] }), "x", "z").tags).toEqual(["z"]);
  });

  it("returns the entry untouched when the old name is absent", () => {
    const before = item("a", { tags: ["x"] });
    expect(rewriteItemTags(before, "missing", "z")).toBe(before);
    expect(rewriteItemTags(item("a"), "x", "z").tags ?? []).toEqual([]);
  });
});

describe("removeItemTag", () => {
  it("removes the named tag", () => {
    expect(removeItemTag(item("a", { tags: ["x", "y"] }), "x").tags).toEqual(["y"]);
  });

  it("returns the entry untouched when the name is absent", () => {
    const before = item("a", { tags: ["x"] });
    expect(removeItemTag(before, "missing")).toBe(before);
  });
});

describe("mergeDeletedHistoryPage", () => {
  it("appends new recycle-bin rows as deleted", () => {
    const merged = mergeDeletedHistoryPage(
      [item("live")],
      [item("gone-1"), item("gone-2")],
      new Set(),
    );
    expect(merged.map((entry) => entry.id)).toEqual(["live", "gone-1", "gone-2"]);
    expect(merged.slice(1).every((entry) => entry.deleted === true)).toBe(true);
  });

  it("does not resurrect a row restored while the page was in flight", () => {
    // The row is already back in the active list (deleted: false); a stale
    // recycle-bin response still lists it and must not flip it back.
    const restored = item("a", { deleted: false, title: "restored" });
    const merged = mergeDeletedHistoryPage(
      [restored],
      [item("a", { deleted: true, title: "stale" })],
      new Set(),
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].deleted).toBe(false);
    expect(merged[0].title).toBe("restored");
  });

  it("refreshes an existing deleted row from the persisted page", () => {
    const merged = mergeDeletedHistoryPage(
      [item("a", { deleted: true, title: "old" })],
      [item("a", { deleted: true, title: "fresh" })],
      new Set(),
    );
    expect(merged).toHaveLength(1);
    expect(merged[0].title).toBe("fresh");
    expect(merged[0].deleted).toBe(true);
  });

  it("drops suppressed ids from both the loaded list and the incoming page", () => {
    const merged = mergeDeletedHistoryPage(
      [item("keep"), item("purged", { deleted: true })],
      [item("purged", { deleted: true }), item("keep")],
      new Set(["purged"]),
    );
    expect(merged.map((entry) => entry.id)).toEqual(["keep"]);
  });
});
