import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import {
  appendItems,
  applyItemPatches,
  closeSearchResults,
  createItemStore,
  mergeSearchCachePage,
  removeItems,
  replaceViewItems,
  setDetailItem,
  type ItemStore,
} from "./item-store";
import Probe from "./item-store-view.probe.svelte";
import { createItemStoreView, type ItemStoreView } from "./item-store-view.svelte";

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

/** One record ("a") displayed by the history, indexed, and cache views. */
function sharedStore(): ItemStore {
  let store = createItemStore([item("a"), item("b")]);
  store = appendItems(store, [item("a")], "indexed");
  store = appendItems(store, [item("a")], "cache");
  return store;
}

interface Snapshot {
  historyIds: string[];
  indexedIds: string[] | null;
  detailId: string | null;
  identityHeld: boolean;
}

const mounted: (() => void)[] = [];
afterEach(() => {
  while (mounted.length) mounted.pop()?.();
});

/**
 * Renders the store view through a real component so the projections are owned
 * `$derived`s, exactly as in the route, and returns the latest render.
 */
function render(initial: ItemStore) {
  const seen: Snapshot[] = [];
  const out: { view?: ItemStoreView } = {};
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(Probe, {
    target,
    props: { initial, out, onchange: (snapshot: Snapshot) => seen.push(snapshot) },
  });
  mounted.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  const view = out.view;
  if (!view) throw new Error("probe did not hand back a view");
  return {
    view,
    seen,
    latest: () => seen[seen.length - 1],
    rendered: () => target.textContent?.trim(),
  };
}

describe("createItemStoreView", () => {
  it("projects every view from the one record map", () => {
    const store = sharedStore();
    const probe = render(store);

    expect(probe.latest().historyIds).toEqual(["a", "b"]);
    expect(probe.latest().indexedIds).toEqual(["a"]);
    expect(probe.latest().detailId).toBeNull();
    expect(probe.rendered()).toBe("2|1|-");
    // One record, one object: the views cannot hold divergent versions.
    expect(probe.latest().identityHeld).toBe(true);
    expect(probe.view.indexed![0]).toBe(probe.view.history[0]);
    expect(store.byId.get("a")).toBe(probe.view.history[0]);
  });

  it("recomputes every projection when the store is replaced", () => {
    const probe = render(sharedStore());

    probe.view.current = applyItemPatches(
      probe.view.current,
      new Map([["a", { title: "patched" }]]),
    );
    flushSync();

    expect(probe.latest().historyIds).toEqual(["a", "b"]);
    expect(probe.latest().indexedIds).toEqual(["a"]);
    expect(probe.view.history[0].title).toBe("patched");
    // The patched record is the single object every view hands out.
    expect(probe.view.indexed![0]).toBe(probe.view.history[0]);
    expect(probe.latest().identityHeld).toBe(true);
  });

  it("keeps an in-place field write invisible to the projections", () => {
    // The deep-proxy contract, exercised through a component: a record handed
    // out by a projection is a plain object, not a live reactive proxy. Writing
    // to it in place cannot reach the store's reactivity, which is why every
    // mutator replaces the whole store instead of editing records (or the Map)
    // in place. Were the record proxied, the write below would be picked up
    // without any store assignment.
    const store = sharedStore();
    const probe = render(store);
    const before = probe.seen.length;

    probe.view.history[0].title = "written-in-place";
    flushSync();

    expect(probe.seen.length).toBe(before);
    expect(probe.view.history[0].title).toBe("written-in-place");

    probe.view.current = applyItemPatches(
      probe.view.current,
      new Map([["a", { title: "patched" }]]),
    );
    flushSync();
    expect(probe.seen.length).toBe(before + 1);
    expect(probe.view.history[0].title).toBe("patched");
  });

  it("keeps 'no search displayed' as null and an emptied search as an empty list", () => {
    const probe = render(appendItems(createItemStore(), [item("z")], "indexed"));
    expect(probe.latest().indexedIds).toEqual(["z"]);

    probe.view.current = closeSearchResults(probe.view.current);
    flushSync();
    expect(probe.latest().indexedIds).toBeNull();
    expect(probe.rendered()).toBe("0|-1|-");

    probe.view.current = replaceViewItems(probe.view.current, [], "indexed");
    flushSync();
    expect(probe.latest().indexedIds).toEqual([]);
    expect(probe.rendered()).toBe("0|0|-");
  });

  it("mirrors removals, detail selection, and cache merges through the projection", () => {
    const probe = render(sharedStore());
    const view = probe.view;

    view.current = mergeSearchCachePage(view.current, {
      results: [item("c")],
      loadedIds: new Set(view.current.historyIds),
      policy: "fifo",
      max: 10,
    });
    view.current = setDetailItem(view.current, item("a"));
    flushSync();
    expect(view.current.cacheIds).toEqual(["a", "c"]);
    expect(probe.latest().detailId).toBe("a");
    expect(probe.rendered()).toBe("2|1|a");

    view.current = removeItems(view.current, new Set(["a"]));
    flushSync();
    expect(probe.latest().historyIds).toEqual(["b"]);
    expect(probe.latest().indexedIds).toEqual([]);
    expect(probe.latest().detailId).toBeNull();
    expect(probe.rendered()).toBe("1|0|-");
  });
});
