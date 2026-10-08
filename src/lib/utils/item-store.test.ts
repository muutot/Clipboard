import { describe, expect, it } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import {
  appendItems,
  applyItemPatches,
  captureAffectedItems,
  clearView,
  closeSearchResults,
  createItemStore,
  findLoadedItem,
  getDetailItem,
  getIndexedItems,
  getItems,
  mergeDeletedHistoryPage,
  mergeSearchCachePage,
  promoteFromCache,
  promoteItem,
  removeItemTag,
  removeItems,
  replaceItem,
  replaceViewItems,
  restoreAffectedItems,
  rewriteItemTags,
  setDetailItem,
  trimLoadedHistory,
  type ItemStore,
} from "./item-store";

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

/**
 * The MAINT-02 invariant, asserted after every operation: `byId` holds exactly
 * the union of the four views, and no view displays an id the map has lost.
 * The old four-copy funnel could only be tested for "the fan-out touched all
 * copies"; here drift is unrepresentable.
 */
function assertConsistent(store: ItemStore) {
  const live = new Set<string>(store.historyIds);
  for (const id of store.indexedIds ?? []) live.add(id);
  for (const id of store.cacheIds) live.add(id);
  if (store.detailId !== null) live.add(store.detailId);

  expect([...store.byId.keys()].sort()).toEqual([...live].sort());
  for (const ids of [store.historyIds, store.indexedIds ?? [], store.cacheIds]) {
    for (const id of ids) expect(store.byId.has(id)).toBe(true);
    // One id may legitimately appear in several views at once — that overlap is
    // exactly what used to drift — but never twice inside the same view.
    expect(new Set(ids).size).toBe(ids.length);
  }
  if (store.detailId !== null) expect(store.byId.has(store.detailId)).toBe(true);
}

/** One record held by all four views at once. */
function shared(): ItemStore {
  let store = createItemStore([item("a"), item("b")]);
  store = appendItems(store, [item("a")], "indexed");
  store = appendItems(store, [item("a")], "cache");
  store = setDetailItem(store, item("a"));
  return store;
}

describe("createItemStore / getItems", () => {
  it("starts empty with no search displayed", () => {
    const store = createItemStore();
    assertConsistent(store);
    expect(getItems(store, "history")).toEqual([]);
    expect(getIndexedItems(store)).toBeNull();
    expect(getDetailItem(store)).toBeNull();
  });

  it("seeds the history view in order", () => {
    const store = createItemStore([item("a"), item("b")]);
    assertConsistent(store);
    expect(getItems(store, "history").map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("materializes every view from the same record", () => {
    const store = shared();
    assertConsistent(store);
    const [history] = getItems(store, "history");
    const [indexed] = getIndexedItems(store)!;
    const [cached] = getItems(store, "cache");
    // Identity, not just equality: the four views cannot hold divergent
    // versions because there is only one record to read.
    expect(indexed).toBe(history);
    expect(cached).toBe(history);
    expect(getDetailItem(store)).toBe(history);
  });
});

describe("applyItemPatches", () => {
  it("updates every view that displays the record in one write", () => {
    // Regression: a tags update once skipped the spare search cache, so search
    // results showed stale tags while the list showed fresh ones.
    const next = applyItemPatches(shared(), new Map([["a", { tags: ["work"] }]]));
    assertConsistent(next);
    for (const items of [
      getItems(next, "history"),
      getIndexedItems(next)!,
      getItems(next, "cache"),
    ]) {
      expect(items.find((i) => i.id === "a")?.tags).toEqual(["work"]);
    }
    expect(getDetailItem(next)?.tags).toEqual(["work"]);
  });

  it("keeps untouched records identical and skips unloaded ids", () => {
    const before = shared();
    const next = applyItemPatches(
      before,
      new Map<string, Partial<ClipboardItem>>([
        ["a", { favorite: true }],
        ["never-loaded", { title: "ghost" }],
      ]),
    );
    assertConsistent(next);
    expect(getItems(next, "history")[1]).toBe(getItems(before, "history")[1]);
    expect(findLoadedItem(next, "never-loaded")).toBeUndefined();
  });

  it("is a no-op for empty patch sets and unloaded-only patch sets", () => {
    const before = shared();
    expect(applyItemPatches(before, new Map())).toBe(before);
    expect(applyItemPatches(before, new Map([["ghost", { title: "x" }]]))).toBe(before);
  });
});

describe("removeItems", () => {
  it("purges ids from the map and every view, and empties the detail pane", () => {
    const next = removeItems(shared(), new Set(["a"]));
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["b"]);
    expect(getIndexedItems(next)).toEqual([]);
    expect(getItems(next, "cache")).toEqual([]);
    expect(getDetailItem(next)).toBeNull();
    expect(findLoadedItem(next, "a")).toBeUndefined();
  });

  it("keeps an unrelated detail item and is a no-op for empty sets", () => {
    const before = setDetailItem(shared(), item("b"));
    const next = removeItems(before, new Set(["a"]));
    assertConsistent(next);
    expect(getDetailItem(next)?.id).toBe("b");
    expect(removeItems(before, new Set())).toBe(before);
    expect(removeItems(before, new Set(["ghost"]))).toBe(before);
  });

  it("keeps 'no search displayed' distinguishable from an emptied search", () => {
    const noSearch = createItemStore([item("a")]);
    assertConsistent(removeItems(noSearch, new Set(["a"])));
    expect(getIndexedItems(removeItems(noSearch, new Set(["a"])))).toBeNull();

    const searching = appendItems(noSearch, [item("b")], "indexed");
    expect(getIndexedItems(removeItems(searching, new Set(["b"])))).toEqual([]);
  });
});

describe("replaceItem / setDetailItem", () => {
  it("swaps the whole record for every view at once", () => {
    const updated = item("a", { title: "materialized" });
    const next = replaceItem(shared(), updated);
    assertConsistent(next);
    expect(getItems(next, "history")[0]).toBe(updated);
    expect(getIndexedItems(next)![0]).toBe(updated);
    expect(getItems(next, "cache")[0]).toBe(updated);
    expect(getDetailItem(next)).toBe(updated);
    expect(getItems(next, "history")[1]).not.toBe(updated);
  });

  it("ignores a record no view displays", () => {
    const before = shared();
    expect(replaceItem(before, item("ghost"))).toBe(before);
  });

  it("releases a record that only the detail pane displayed", () => {
    const detailOnly = setDetailItem(createItemStore(), item("solo"));
    expect(findLoadedItem(detailOnly, "solo")).toBeDefined();
    const cleared = setDetailItem(detailOnly, null);
    assertConsistent(cleared);
    expect(findLoadedItem(cleared, "solo")).toBeUndefined();
    expect(setDetailItem(cleared, null)).toBe(cleared);
  });
});

describe("appendItems / promoteItem / clearView", () => {
  it("appends unseen ids, keeps known positions, and refreshes their content", () => {
    const before = createItemStore([item("a"), item("b")]);
    const next = appendItems(before, [item("b", { title: "fresh" }), item("c")], "history");
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b", "c"]);
    expect(getItems(next, "history")[1].title).toBe("fresh");
    expect(appendItems(next, [], "history")).toBe(next);
  });

  it("switches the search view on when a first result page is appended", () => {
    const next = appendItems(createItemStore([item("a")]), [item("z")], "indexed");
    assertConsistent(next);
    expect(getIndexedItems(next)?.map((i) => i.id)).toEqual(["z"]);
  });

  it("pins a re-copied record to the top without disturbing other views", () => {
    const before = createItemStore([item("a"), item("b"), item("c")]);
    const promoted = item("c", { title: "re-copied" });
    const next = promoteItem(before, promoted, "history");
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["c", "a", "b"]);
    expect(getItems(next, "history")[0]).toBe(promoted);
  });

  it("empties a view and releases only the records it alone displayed", () => {
    const before = shared();
    const cleared = clearView(before, "cache");
    assertConsistent(cleared);
    expect(getItems(cleared, "cache")).toEqual([]);
    // "a" is still in the history and indexed views; "b" was never cached.
    expect(findLoadedItem(cleared, "a")).toBeDefined();
    // An already-empty view has nothing to release and stays identical.
    const empty = createItemStore([item("a")]);
    expect(clearView(empty, "indexed")).toBe(empty);
    expect(clearView(empty, "history")).not.toBe(empty);
  });

  it("releases the previous detail record when the pane is re-pointed", () => {
    // A record can end up displayed by the pane alone — a spare-cache id the
    // user opened, whose cache entry was later promoted. Re-pointing the pane
    // must not leave it in the map forever.
    const cached = appendItems(createItemStore(), [item("solo")], "cache");
    const before = clearView(setDetailItem(cached, findLoadedItem(cached, "solo")!), "cache");
    expect(findLoadedItem(before, "solo")).toBeDefined();
    const next = setDetailItem(before, item("other"));
    assertConsistent(next);
    expect(findLoadedItem(next, "solo")).toBeUndefined();
    expect(getDetailItem(next)?.id).toBe("other");
  });
});

describe("captureAffectedItems / restoreAffectedItems", () => {
  it("rolls back only the affected ids and keeps records that arrived meanwhile", () => {
    // The persist call is async, so a capture can land while it is in flight.
    const before = appendItems(shared(), [item("c")], "history");
    const snapshot = captureAffectedItems(before, new Set(["a", "b"]));

    // Mutation removes "b" and flips "a"; meanwhile a new capture arrives.
    const during = applyItemPatches(
      appendItems(
        removeItems(setDetailItem(before, null), new Set(["b"])),
        [item("new")],
        "history",
      ),
      new Map([["a", { favorite: true }]]),
    );

    const next = restoreAffectedItems(during, snapshot);
    assertConsistent(next);

    // "new" survived: a whole-array rollback would have dropped it, because it
    // is absent from any pre-mutation snapshot. "b" is re-inserted at the index
    // it held and "a" is patched in place rather than moved, so nothing the
    // user can currently see is reordered behind their back.
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b", "c", "new"]);
    expect(findLoadedItem(next, "a")?.favorite).toBe(false);
  });

  it("restores each view's own recorded position", () => {
    const before = createItemStore([item("a"), item("b"), item("c")]);
    const seeded = appendItems(appendItems(before, [item("b")], "indexed"), [item("b")], "cache");
    const snapshot = captureAffectedItems(seeded, new Set(["b"]));

    const during = removeItems(removeItems(seeded, new Set(["b"])), new Set(["b"]));
    expect(getItems(during, "cache")).toEqual([]);

    const next = restoreAffectedItems(during, snapshot);
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "b", "c"]);
    expect(getIndexedItems(next)!.map((i) => i.id)).toEqual(["b"]);
    // "b" sat alone in the cache, so it returns to index 0 there rather than at
    // the index it held in the history list.
    expect(getItems(next, "cache").map((i) => i.id)).toEqual(["b"]);
  });

  it("captures and reclaims a record only the detail pane displayed", () => {
    const before = setDetailItem(createItemStore([item("a")]), item("solo"));
    const snapshot = captureAffectedItems(before, new Set(["solo"]));
    expect(snapshot.detailId).toBe("solo");

    const cleared = restoreAffectedItems(setDetailItem(before, null), snapshot);
    assertConsistent(cleared);
    expect(getDetailItem(cleared)?.id).toBe("solo");
  });

  it("reclaims an emptied detail pane but not one the user re-pointed", () => {
    const before = setDetailItem(createItemStore([item("a"), item("b")]), item("a"));
    const snapshot = captureAffectedItems(before, new Set(["a"]));

    const userOpenedAnother = restoreAffectedItems(setDetailItem(before, item("b")), snapshot);
    expect(getDetailItem(userOpenedAnother)?.id).toBe("b");
  });

  it("records only the selected ids it was given", () => {
    const before = createItemStore([item("a"), item("b")]);
    const snapshot = captureAffectedItems(before, new Set(["a", "b"]), new Set(["b", "other"]));
    expect([...snapshot.selected]).toEqual(["b"]);
  });

  it("is a no-op for an empty capture", () => {
    const before = shared();
    const snapshot = captureAffectedItems(before, new Set());
    expect(snapshot.previous.size).toBe(0);
    expect(restoreAffectedItems(before, snapshot)).toBe(before);
  });
});

describe("rewriteItemTags / removeItemTag", () => {
  it("renames the tag and drops duplicates the rename would create", () => {
    expect(rewriteItemTags(item("a", { tags: ["x", "y"] }), "x", "z").tags).toEqual(["z", "y"]);
    expect(rewriteItemTags(item("a", { tags: ["x", "z"] }), "x", "z").tags).toEqual(["z"]);
  });

  it("returns the record untouched when the old name is absent", () => {
    const before = item("a", { tags: ["x"] });
    expect(rewriteItemTags(before, "missing", "z")).toBe(before);
    expect(rewriteItemTags(item("a"), "x", "z").tags ?? []).toEqual([]);
  });

  it("removes the named tag", () => {
    expect(removeItemTag(item("a", { tags: ["x", "y"] }), "x").tags).toEqual(["y"]);
    const before = item("a", { tags: ["x"] });
    expect(removeItemTag(before, "missing")).toBe(before);
  });
});

describe("mergeDeletedHistoryPage", () => {
  it("appends new recycle-bin rows as deleted", () => {
    const next = mergeDeletedHistoryPage(
      createItemStore([item("live")]),
      [item("gone-1"), item("gone-2")],
      new Set(),
    );
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["live", "gone-1", "gone-2"]);
    expect(
      getItems(next, "history")
        .slice(1)
        .every((i) => i.deleted),
    ).toBe(true);
  });

  it("does not resurrect a row restored while the page was in flight", () => {
    // The row is already back in the list (deleted: false); a stale recycle-bin
    // response still lists it and must not flip it back.
    const next = mergeDeletedHistoryPage(
      createItemStore([item("a", { deleted: false, title: "restored" })]),
      [item("a", { deleted: true, title: "stale" })],
      new Set(),
    );
    expect(getItems(next, "history")).toHaveLength(1);
    expect(getItems(next, "history")[0].deleted).toBe(false);
    expect(getItems(next, "history")[0].title).toBe("restored");
  });

  it("refreshes an existing deleted row from the persisted page", () => {
    const next = mergeDeletedHistoryPage(
      createItemStore([item("a", { deleted: true, title: "old" })]),
      [item("a", { deleted: true, title: "fresh" })],
      new Set(),
    );
    expect(getItems(next, "history")).toHaveLength(1);
    expect(getItems(next, "history")[0].title).toBe("fresh");
    expect(getItems(next, "history")[0].deleted).toBe(true);
  });

  it("drops suppressed ids from both the loaded list and the incoming page", () => {
    const next = mergeDeletedHistoryPage(
      createItemStore([item("keep"), item("purged", { deleted: true })]),
      [item("purged", { deleted: true }), item("keep")],
      new Set(["purged"]),
    );
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["keep"]);
  });
});

describe("trimLoadedHistory", () => {
  const history = (count: number) =>
    createItemStore(
      Array.from({ length: count }, (_, index) => item(`i${index}`, { createdAt: index })),
    );

  it("is a no-op below the threshold", () => {
    const before = history(5);
    expect(trimLoadedHistory(before, { limit: 10, tolerance: 2 })).toBe(before);
  });

  it("releases at most `tolerance` oldest ordinary records", () => {
    const next = trimLoadedHistory(history(6), { limit: 3, tolerance: 2 });
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["i2", "i3", "i4", "i5"]);
  });

  it("never releases favorites or recycle-bin rows", () => {
    const seeded = createItemStore([
      item("old-fav", { createdAt: 0, favorite: true }),
      item("old-deleted", { createdAt: 1, deleted: true }),
      item("plain-1", { createdAt: 2 }),
      item("plain-2", { createdAt: 3 }),
      item("plain-3", { createdAt: 4 }),
    ]);
    // Reuse the seeded record so the favorite flag is not overwritten.
    const before = setDetailItem(seeded, findLoadedItem(seeded, "old-fav")!);
    const next = trimLoadedHistory(before, { limit: 2, tolerance: 2 });
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual([
      "old-fav",
      "old-deleted",
      "plain-3",
    ]);
    expect(getDetailItem(next)?.id).toBe("old-fav");
  });

  it("keeps a record the detail pane alone still displays", () => {
    const before = setDetailItem(
      createItemStore([item("a", { createdAt: 0 }), item("b", { createdAt: 1 })]),
      item("solo", { createdAt: 2 }),
    );
    const next = trimLoadedHistory(before, { limit: 0, tolerance: 1 });
    assertConsistent(next);
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["b"]);
    expect(getDetailItem(next)?.id).toBe("solo");
  });

  it("keeps everything when no ordinary record is evictable", () => {
    const before = createItemStore([
      item("fav", { favorite: true }),
      item("del", { deleted: true }),
    ]);
    expect(trimLoadedHistory(before, { limit: 0, tolerance: 1 })).toBe(before);
  });
});

describe("mergeSearchCachePage", () => {
  it("applies a reduced cache cap without waiting for another result page", () => {
    const before = cached("a", "b", "c");
    const next = mergeSearchCachePage(before, {
      results: [],
      loadedIds: new Set(),
      policy: "fifo",
      max: 1,
    });
    expect(next.cacheIds).toEqual(["c"]);
    expect(next.byId.has("a")).toBe(false);
  });
  const cached = (...ids: string[]) => {
    let store = createItemStore();
    for (const id of ids) store = appendItems(store, [item(id)], "cache");
    return store;
  };

  it("drops results the loaded history already displays and inserts the rest in order", () => {
    const store = createItemStore([item("loaded")]);
    const next = mergeSearchCachePage(store, {
      results: [item("loaded"), item("new-1"), item("new-2")],
      loadedIds: new Set(["loaded"]),
      policy: "fifo",
      max: 10,
    });
    assertConsistent(next);
    expect(getItems(next, "cache").map((i) => i.id)).toEqual(["new-1", "new-2"]);
    // Dropping the cache copy must not evict the record the history owns.
    expect(findLoadedItem(next, "loaded")).toBeDefined();
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["loaded"]);
  });

  it("refreshes cached content without reordering under fifo", () => {
    const next = mergeSearchCachePage(cached("a", "b"), {
      results: [item("a", { title: "updated" })],
      loadedIds: new Set(),
      policy: "fifo",
      max: 10,
    });
    assertConsistent(next);
    expect(getItems(next, "cache").map((i) => i.id)).toEqual(["a", "b"]);
    expect(getItems(next, "cache")[0].title).toBe("updated");
  });

  it("bumps recency of a re-encountered id under lru but not fifo", () => {
    const base = cached("old", "mid");
    const results = [item("old")];
    expect(
      getItems(
        mergeSearchCachePage(base, { results, loadedIds: new Set(), policy: "fifo", max: 10 }),
        "cache",
      ).map((i) => i.id),
    ).toEqual(["old", "mid"]);
    expect(
      getItems(
        mergeSearchCachePage(base, { results, loadedIds: new Set(), policy: "lru", max: 10 }),
        "cache",
      ).map((i) => i.id),
    ).toEqual(["mid", "old"]);
  });

  it("evicts least recently used entries beyond the maximum", () => {
    let store = createItemStore();
    for (let round = 0; round < 5; round += 1) {
      store = mergeSearchCachePage(store, {
        results: [item(`n${round}`)],
        loadedIds: new Set(),
        policy: "fifo",
        max: 3,
      });
    }
    assertConsistent(store);
    expect(getItems(store, "cache").map((i) => i.id)).toEqual(["n2", "n3", "n4"]);
  });

  it("re-inserts an evicted id when the backend returns it again", () => {
    let store = createItemStore();
    for (let round = 0; round < 4; round += 1) {
      store = mergeSearchCachePage(store, {
        results: [item(`n${round}`)],
        loadedIds: new Set(),
        policy: "fifo",
        max: 2,
      });
    }
    store = mergeSearchCachePage(store, {
      results: [item("n1")],
      loadedIds: new Set(),
      policy: "fifo",
      max: 2,
    });
    assertConsistent(store);
    expect(getItems(store, "cache").map((i) => i.id)).toEqual(["n3", "n1"]);
  });

  it("is a no-op for an empty result page", () => {
    const before = cached("a");
    expect(
      mergeSearchCachePage(before, {
        results: [],
        loadedIds: new Set(),
        policy: "lru",
        max: 1,
      }),
    ).toBe(before);
  });
});

describe("promoteFromCache", () => {
  it("removes promoted ids from the cache and keeps the loaded records", () => {
    const before = createItemStore([item("a"), item("c")]);
    const seeded = appendItems(appendItems(before, [item("b")], "cache"), [item("c")], "cache");
    const next = promoteFromCache(seeded, new Set(["a", "c"]));
    assertConsistent(next);
    expect(getItems(next, "cache").map((i) => i.id)).toEqual(["b"]);
    // "a" and "c" are loaded history rows; only their cache copies went away.
    expect(getItems(next, "history").map((i) => i.id)).toEqual(["a", "c"]);
  });

  it("returns the same store for empty or non-matching sets", () => {
    const before = appendItems(createItemStore(), [item("a")], "cache");
    expect(promoteFromCache(before, new Set())).toBe(before);
    expect(promoteFromCache(before, new Set(["missing"]))).toBe(before);
  });
});

describe("closeSearchResults / replaceViewItems", () => {
  it("distinguishes no search from a search with no rows", () => {
    const searching = appendItems(createItemStore([item("a")]), [item("z")], "cache");
    const withResults = appendItems(searching, [item("z")], "indexed");
    assertConsistent(withResults);

    const hidden = closeSearchResults(withResults);
    assertConsistent(hidden);
    expect(getIndexedItems(hidden)).toBeNull();
    // The result is still in the spare cache, so it can come back.
    expect(getItems(hidden, "cache").map((i) => i.id)).toEqual(["z"]);
    expect(closeSearchResults(hidden)).toBe(hidden);

    const emptied = replaceViewItems(hidden, [], "indexed");
    expect(getIndexedItems(emptied)).toEqual([]);
  });

  it("replaces a view's contents and releases what it dropped", () => {
    const before = appendItems(createItemStore(), [item("old")], "cache");
    const next = replaceViewItems(before, [item("new")], "cache");
    assertConsistent(next);
    expect(getItems(next, "cache").map((i) => i.id)).toEqual(["new"]);
    expect(findLoadedItem(next, "old")).toBeUndefined();
  });
});

describe("single source of truth under mixed operations", () => {
  it("holds the union invariant and view identity across a long edit sequence", () => {
    // Deterministic pseudo-random walk: every operation is one the route can
    // perform, and after each one the store must still satisfy the invariant.
    let seed = 20260930;
    const nextInt = (bound: number) => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed % bound;
    };
    const ids = Array.from({ length: 12 }, (_, index) => `i${index}`);

    let store = createItemStore([item(ids[0], { createdAt: 0 })]);
    for (let step = 0; step < 400; step += 1) {
      const target = item(ids[nextInt(ids.length)], { createdAt: nextInt(50) });
      switch (nextInt(9)) {
        case 0:
          store = appendItems(store, [target], "history");
          break;
        case 1:
          store = appendItems(store, [target], "indexed");
          break;
        case 2:
          store = appendItems(store, [target], "cache");
          break;
        case 3:
          store = promoteItem(store, target, "history");
          break;
        case 4:
          store = applyItemPatches(store, new Map([[target.id, { title: `t${step}` }]]));
          break;
        case 5:
          store = removeItems(store, new Set([target.id]));
          break;
        case 6:
          store = replaceItem(store, target);
          break;
        case 7:
          store = setDetailItem(store, nextInt(2) === 0 ? target : null);
          break;
        default:
          store = trimLoadedHistory(store, { limit: 4, tolerance: 2 });
          break;
      }
      assertConsistent(store);
    }

    // The views still agree on content because they read the one record.
    for (const view of ["history", "indexed", "cache"] as const) {
      const rendered = getItems(store, view);
      for (const record of rendered) {
        expect(findLoadedItem(store, record.id)).toBe(record);
      }
    }
  });
});
