import { describe, expect, it } from "vitest";
import {
  mergeSearchCachePage,
  promoteFromCache,
  trimLoadedItems,
  type SearchCacheState,
} from "./search-cache";
import type { ClipboardItem } from "$lib/types/clipboard";

function item(id: string, overrides: Partial<ClipboardItem> = {}): ClipboardItem {
  return {
    id,
    kind: "text",
    title: id,
    preview: "",
    sourceApp: "Notepad",
    sourceTone: "neutral",
    sizeLabel: "",
    createdAt: 1000,
    favorite: false,
    ...overrides,
  };
}

const emptyState: SearchCacheState = { cache: [], accessOrder: [] };

describe("mergeSearchCachePage", () => {
  it("drops results that are already loaded and inserts fresh ones in order", () => {
    const state = mergeSearchCachePage(emptyState, {
      results: [item("loaded"), item("new-1"), item("new-2")],
      loadedIds: new Set(["loaded"]),
      policy: "fifo",
      max: 10,
    });
    expect(state.accessOrder).toEqual(["new-1", "new-2"]);
    expect(state.cache.map((entry) => entry.id)).toEqual(["new-1", "new-2"]);
  });

  it("refreshes content of cached ids without reordering under fifo", () => {
    const state = mergeSearchCachePage(
      { cache: [item("a"), item("b")], accessOrder: ["a", "b"] },
      {
        results: [item("a", { title: "updated" })],
        loadedIds: new Set(),
        policy: "fifo",
        max: 10,
      },
    );
    expect(state.accessOrder).toEqual(["a", "b"]);
    expect(state.cache.find((entry) => entry.id === "a")?.title).toBe("updated");
  });

  it("bumps recency of cached ids under lru but not fifo", () => {
    const cache = [item("old"), item("mid")];
    const accessOrder = ["old", "mid"];
    const results = [item("old")];

    const fifo = mergeSearchCachePage(
      { cache, accessOrder },
      { results, loadedIds: new Set(), policy: "fifo", max: 10 },
    );
    expect(fifo.accessOrder).toEqual(["old", "mid"]);

    const lru = mergeSearchCachePage(
      { cache, accessOrder },
      { results, loadedIds: new Set(), policy: "lru", max: 10 },
    );
    expect(lru.accessOrder).toEqual(["mid", "old"]);
  });

  it("evicts least recently used entries beyond the maximum", () => {
    let state: SearchCacheState = emptyState;
    for (let round = 0; round < 5; round++) {
      state = mergeSearchCachePage(state, {
        results: [item(`n${round}`)],
        loadedIds: new Set(),
        policy: "fifo",
        max: 3,
      });
    }
    // n0 and n1 were evicted; the three most recent survive.
    expect(state.cache.map((entry) => entry.id)).toEqual(["n2", "n3", "n4"]);
    expect(state.accessOrder).toEqual(["n2", "n3", "n4"]);
  });

  it("re-inserts an evicted id when the backend returns it again", () => {
    let state: SearchCacheState = emptyState;
    for (let round = 0; round < 4; round++) {
      state = mergeSearchCachePage(state, {
        results: [item(`n${round}`)],
        loadedIds: new Set(),
        policy: "fifo",
        max: 2,
      });
    }
    // After four single-item pages against a max of two, only the newest
    // pair survives.
    expect(state.accessOrder).toEqual(["n2", "n3"]);
    state = mergeSearchCachePage(state, {
      results: [item("n1")],
      loadedIds: new Set(),
      policy: "fifo",
      max: 2,
    });
    expect(state.accessOrder).toEqual(["n3", "n1"]);
  });
});

describe("promoteFromCache", () => {
  it("removes promoted ids from both lists", () => {
    const state: SearchCacheState = {
      cache: [item("a"), item("b"), item("c")],
      accessOrder: ["a", "b", "c"],
    };
    const next = promoteFromCache(state, new Set(["a", "c"]));
    expect(next.cache.map((entry) => entry.id)).toEqual(["b"]);
    expect(next.accessOrder).toEqual(["b"]);
  });

  it("returns the original state for empty or non-matching sets", () => {
    const state: SearchCacheState = { cache: [item("a")], accessOrder: ["a"] };
    expect(promoteFromCache(state, new Set())).toBe(state);
    expect(promoteFromCache(state, new Set(["missing"]))).toBe(state);
  });
});

describe("trimLoadedItems", () => {
  function row(id: string, createdAt: number, overrides: Partial<ClipboardItem> = {}) {
    return item(id, { createdAt, ...overrides });
  }

  it("returns the same list while within limit plus tolerance", () => {
    const items = [row("a", 1), row("b", 2)];
    expect(trimLoadedItems(items, 100, 10)).toBe(items);
  });

  it("evicts oldest non-favorite non-deleted entries first", () => {
    const items = [
      row("fav-old", 1, { favorite: true }),
      row("deleted-old", 2, { deleted: true }),
      row("old-1", 3),
      row("old-2", 4),
      row("old-3", 5),
      row("fresh", 100),
    ];
    // Limit 2 + tolerance 3: list holds 6 > 5, so the 3 oldest evictable
    // rows go — favorites and recycle-bin rows are protected.
    const result = trimLoadedItems(items, 2, 3);
    expect(result.map((entry) => entry.id)).toEqual(["fav-old", "deleted-old", "fresh"]);
  });

  it("keeps everything when only protected rows would need eviction", () => {
    const items = [
      row("fav-1", 1, { favorite: true }),
      row("fav-2", 2, { favorite: true }),
      row("del-1", 3, { deleted: true }),
    ];
    expect(trimLoadedItems(items, 1, 1)).toHaveLength(3);
  });
});
