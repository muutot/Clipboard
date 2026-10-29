import { describe, expect, it, vi } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import { createItemMutations, type ItemMetadataPatch } from "./item-mutations";
import {
  applyItemPatches,
  createItemStore,
  getItems,
  removeItems,
  replaceItem,
} from "./item-store";
import type { ItemStoreView } from "./item-store-view.svelte";

function record(id = "a"): ClipboardItem {
  return {
    id,
    kind: "text",
    title: "original",
    tags: ["original"],
    customTitle: false,
    preview: "body",
    favorite: false,
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "4 B",
    createdAt: 1,
  };
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture() {
  let store = createItemStore([record("a"), record("b")]);
  let mutations: ReturnType<typeof createItemMutations>;
  const view: ItemStoreView = {
    get current() {
      return store;
    },
    set current(next) {
      store = mutations.observe(store, next);
    },
    get history() {
      return getItems(store, "history");
    },
    get indexed() {
      return null;
    },
    get detail() {
      return null;
    },
  };
  mutations = createItemMutations(() => view);
  return {
    view,
    mutations,
    row: () => view.current.byId.get("a")!,
    canonical(next: ClipboardItem) {
      mutations.acceptRecords([next]);
      view.current = replaceItem(view.current, next);
    },
  };
}
async function settle() {
  for (let i = 0; i < 16; i++) await Promise.resolve();
}

describe("ordered optimistic item metadata", () => {
  it.each([true, false])(
    "keeps same-valued confirmed membership after an old reply (%s)",
    async (ok) => {
      const f = fixture();
      const gate = deferred<boolean>();
      const result = Promise.allSettled([
        f.mutations.run(new Map([["a", { deleted: true }]]), () => gate.promise),
      ]);
      await settle();
      f.mutations.acceptMembership(["a"], []);
      f.view.current = applyItemPatches(f.view.current, new Map([["a", { deleted: true }]]));
      gate.resolve(ok);
      await result;
      expect(f.row().deleted).toBe(true);
    },
  );

  it("does not infer membership from a full content event", async () => {
    const f = fixture();
    const gate = deferred<boolean>();
    const result = Promise.allSettled([
      f.mutations.run(new Map([["a", { deleted: true }]]), () => gate.promise),
    ]);
    await settle();
    f.mutations.acceptRecords([{ ...record(), title: "saved content" }]);
    f.view.current = replaceItem(f.view.current, { ...f.row(), title: "saved content" });
    expect(f.row().deleted).toBe(true);
    gate.resolve(false);
    await result;
    expect(f.row()).toMatchObject({ deleted: false, title: "saved content" });
  });

  it.each([true, false])(
    "accepts same-valued membership from an identified native page (%s)",
    async (deleted) => {
      const f = fixture();
      f.view.current = applyItemPatches(f.view.current, new Map([["a", { deleted: !deleted }]]));
      const gate = deferred<boolean>();
      const result = Promise.allSettled([
        f.mutations.run(new Map([["a", { deleted }]]), () => gate.promise),
      ]);
      await settle();
      f.mutations.acceptRecords([record()], deleted);
      f.view.current = replaceItem(f.view.current, { ...record(), deleted });
      gate.resolve(false);
      await result;
      expect(f.row().deleted).toBe(deleted);
    },
  );

  it("preserves queued restore intent over an acknowledged deletion then falls back on failure", async () => {
    const f = fixture();
    const deleteGate = deferred<boolean>();
    const restoreGate = deferred<boolean>();
    const restore = vi.fn(() => restoreGate.promise);
    const result = Promise.allSettled([
      f.mutations.run(new Map([["a", { deleted: true }]]), () => deleteGate.promise),
      f.mutations.run(new Map([["a", { deleted: false }]]), restore),
    ]);
    await settle();
    f.mutations.acceptMembership(["a"], []);
    f.view.current = applyItemPatches(f.view.current, new Map([["a", { deleted: true }]]));
    expect(f.row().deleted).toBe(false);
    expect(restore).not.toHaveBeenCalled();
    deleteGate.resolve(true);
    await settle();
    expect(restore).toHaveBeenCalledOnce();
    restoreGate.resolve(false);
    await result;
    expect(f.row().deleted).toBe(true);
  });

  it.each(["favorite", "tags", "title"] as const)(
    "returns to confirmed %s after consecutive failures",
    async (field) => {
      const f = fixture();
      const first = deferred<boolean>();
      const second = deferred<boolean>();
      const values = {
        favorite: [true, false],
        tags: [["first"], ["second"]],
        title: ["first", "second"],
      };
      const before = f.row()[field];
      const run1 = f.mutations.run(
        new Map([["a", { [field]: values[field][0] }]]),
        () => first.promise,
      );
      const run2 = f.mutations.run(
        new Map([["a", { [field]: values[field][1] }]]),
        () => second.promise,
      );
      const results = Promise.allSettled([run1, run2]);
      expect(f.row()[field]).toEqual(values[field][1]);
      await settle();
      first.reject(new Error("first failed"));
      await settle();
      expect(f.row()[field]).toEqual(values[field][1]);
      second.resolve(false);
      await results;
      expect(f.row()[field]).toEqual(before);
    },
  );

  it("keeps a successful same-value reaffirmation after an older failure", async () => {
    const f = fixture();
    const first = deferred<boolean>();
    const save2 = vi.fn(async () => true);
    const run1 = f.mutations.run(new Map([["a", { favorite: true }]]), () => first.promise);
    const run2 = f.mutations.run(new Map([["a", { favorite: true }]]), save2);
    const results = Promise.allSettled([run1, run2]);
    await settle();
    expect(save2).not.toHaveBeenCalled();
    first.reject(new Error("failed"));
    await results;
    expect(save2).toHaveBeenCalledOnce();
    expect(f.row().favorite).toBe(true);
  });

  it("restores a successful baseline when the subsequent request fails", async () => {
    const f = fixture();
    const first = deferred<boolean>();
    const second = deferred<boolean>();
    const run1 = f.mutations.run(new Map([["a", { favorite: true }]]), () => first.promise);
    const run2 = f.mutations.run(new Map([["a", { favorite: false }]]), () => second.promise);
    const results = Promise.allSettled([run1, run2]);
    await settle();
    first.resolve(true);
    await settle();
    second.resolve(false);
    await results;
    expect(f.row().favorite).toBe(true);
  });

  it.each([true, false])(
    "preserves a newer canonical field when an old request settles (%s)",
    async (success) => {
      const f = fixture();
      const gate = deferred<boolean>();
      const run = f.mutations.run(new Map([["a", { favorite: true }]]), () => gate.promise);
      const result = Promise.allSettled([run]);
      await settle();
      f.canonical({ ...record(), title: "newer", favorite: false });
      expect(f.row().favorite).toBe(false);
      gate.resolve(success);
      await result;
      expect(f.row()).toMatchObject({ title: "newer", favorite: false });
    },
  );

  it("recognizes a same-valued authoritative update before a failed request returns", async () => {
    const f = fixture();
    const gate = deferred<boolean>();
    const run = f.mutations.run(new Map([["a", { favorite: true }]]), () => gate.promise);
    const result = Promise.allSettled([run]);
    await settle();
    f.canonical({ ...record(), favorite: true });
    gate.resolve(false);
    await result;
    expect(f.row().favorite).toBe(true);
  });

  it("preserves unsent local intent above a canonical update while serializing a batch", async () => {
    const f = fixture();
    const first = deferred<boolean>();
    const batch = deferred<boolean>();
    const sendBatch = vi.fn(() => batch.promise);
    const run1 = f.mutations.run(new Map([["a", { favorite: true }]]), () => first.promise);
    const run2 = f.mutations.run(
      new Map([
        ["a", { favorite: false }],
        ["b", { favorite: true }],
      ]),
      sendBatch,
    );
    const results = Promise.allSettled([run1, run2]);
    await settle();
    f.canonical({ ...record(), favorite: true });
    expect(f.row().favorite).toBe(false);
    expect(sendBatch).not.toHaveBeenCalled();
    first.resolve(true);
    await settle();
    expect(sendBatch).toHaveBeenCalledOnce();
    batch.resolve(false);
    await results;
    expect(f.row().favorite).toBe(true);
    expect(f.view.current.byId.get("b")!.favorite).toBe(false);
  });

  it("allows disjoint rows to save concurrently", async () => {
    const f = fixture();
    const first = deferred<boolean>();
    const other = vi.fn(async () => true);
    const run1 = f.mutations.run(new Map([["a", { favorite: true }]]), () => first.promise);
    const run2 = f.mutations.run(new Map([["b", { favorite: true }]]), other);
    await run2;
    expect(other).toHaveBeenCalledOnce();
    expect(f.view.current.byId.get("b")!.favorite).toBe(true);
    first.resolve(true);
    await run1;
  });

  it("does not confirm an optimistic favorite through an unrelated partial update", async () => {
    const f = fixture();
    const gate = deferred<boolean>();
    const run = f.mutations.run(new Map([["a", { favorite: true }]]), () => gate.promise);
    const result = Promise.allSettled([run]);
    await settle();
    f.view.current = applyItemPatches(
      f.view.current,
      new Map([["a", { ocrStatus: "completed", lastUsedAtMs: 42 }]]),
    );
    gate.resolve(false);
    await result;
    expect(f.row()).toMatchObject({ favorite: false, ocrStatus: "completed", lastUsedAtMs: 42 });
  });

  it("does not turn a pending tag draft into the baseline of a tag rename", async () => {
    const f = fixture();
    const gate = deferred<boolean>();
    const run = f.mutations.run(new Map([["a", { tags: ["pending"] }]]), () => gate.promise);
    const result = Promise.allSettled([run]);
    await settle();
    expect(f.mutations.confirmedItem(f.row()).tags).toEqual(["original"]);
    f.view.current = applyItemPatches(f.view.current, new Map([["a", { tags: ["renamed"] }]]));
    gate.resolve(false);
    await result;
    expect(f.row().tags).toEqual(["renamed"]);
  });

  it.each([true, false])(
    "does not reinsert a removed record on late completion (%s)",
    async (ok) => {
      const f = fixture();
      const gate = deferred<boolean>();
      const run = f.mutations.run(new Map([["a", { favorite: true }]]), () => gate.promise);
      const result = Promise.allSettled([run]);
      await settle();
      f.view.current = removeItems(f.view.current, new Set(["a"]));
      gate.resolve(ok);
      await result;
      expect(f.view.current.byId.has("a")).toBe(false);
      expect(f.view.current.historyIds).toEqual(["b"]);
    },
  );

  it("releases completed metadata state before the next edit", async () => {
    const f = fixture();
    await f.mutations.run(new Map([["a", { favorite: true }]]), async () => true);
    f.view.current = applyItemPatches(f.view.current, new Map([["a", { favorite: false }]]));
    await expect(
      f.mutations.run(new Map([["a", { favorite: true }]]), async () => false),
    ).rejects.toThrow();
    expect(f.row().favorite).toBe(false);
  });

  it("ignores active replies and cancels queued writes after disposal", async () => {
    const f = fixture();
    const gate = deferred<boolean>();
    const send2 = vi.fn(async () => true);
    const run1 = f.mutations.run(new Map([["a", { favorite: true }]]), () => gate.promise);
    const run2 = f.mutations.run(new Map([["a", { favorite: false }]]), send2);
    const results = Promise.allSettled([run1, run2]);
    await settle();
    const current = f.view.current;
    f.mutations.destroy();
    gate.resolve(true);
    await results;
    expect(f.view.current).toBe(current);
    expect(send2).not.toHaveBeenCalled();
    await expect(f.mutations.run(new Map<string, ItemMetadataPatch>(), send2)).rejects.toThrow(
      "disposed",
    );
  });
});
