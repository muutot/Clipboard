import { describe, expect, it } from "vitest";
import { captureBulkSnapshot, planBulkDelete, setDeletedFlags } from "./bulk-actions";
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

describe("planBulkDelete", () => {
  it("routes deleted rows to permanent and active rows by recycle-bin mode", () => {
    const selection = [
      item("active"),
      item("deleted-row", { deleted: true }),
      item("fav-active", { favorite: true }),
    ];
    expect(planBulkDelete(selection, true)).toEqual({
      softIds: ["active"],
      permanentIds: ["deleted-row"],
      hardIds: [],
    });
    expect(planBulkDelete(selection, false)).toEqual({
      softIds: [],
      permanentIds: ["deleted-row"],
      hardIds: ["active"],
    });
  });

  it("never plans a delete for an active favorite", () => {
    const plan = planBulkDelete([item("fav", { favorite: true })], false);
    expect(plan).toEqual({ softIds: [], permanentIds: [], hardIds: [] });
  });

  it("returns empty buckets for an empty selection", () => {
    expect(planBulkDelete([], true)).toEqual({
      softIds: [],
      permanentIds: [],
      hardIds: [],
    });
  });
});

describe("captureBulkSnapshot", () => {
  it("copies lists and sets so later mutations never leak into the snapshot", () => {
    const items = [item("a"), item("b")];
    const indexed = [item("a")];
    const selected = new Set(["a"]);
    const detail = item("a");
    const snapshot = captureBulkSnapshot({
      items,
      indexedItems: indexed,
      selectedIds: selected,
      detailItem: detail,
    });

    items.push(item("c"));
    items[0].title = "mutated";
    indexed.pop();
    selected.add("b");
    detail.title = "mutated";

    expect(snapshot.items.map((entry) => entry.id)).toEqual(["a", "b"]);
    expect(snapshot.items[0].title).toBe("a");
    expect(snapshot.indexedItems).toHaveLength(1);
    expect(snapshot.selectedIds).toEqual(new Set(["a"]));
    expect(snapshot.detailItem?.title).toBe("a");
  });

  it("keeps null indexed/detail as null", () => {
    const snapshot = captureBulkSnapshot({
      items: [],
      indexedItems: null,
      selectedIds: new Set(),
      detailItem: null,
    });
    expect(snapshot.indexedItems).toBeNull();
    expect(snapshot.detailItem).toBeNull();
  });
});

describe("setDeletedFlags", () => {
  it("flags only the requested ids and leaves others untouched", () => {
    const items = [item("a"), item("b"), item("c")];
    const result = setDeletedFlags(items, new Set(["b"]), true);
    expect(result.map((entry) => entry.deleted ?? false)).toEqual([false, true, false]);
    // Original list is not mutated.
    expect(items.every((entry) => !entry.deleted)).toBe(true);
  });

  it("reuses the same object when the flag already matches (no-op entries)", () => {
    const target = item("a", { deleted: true });
    const items = [target, item("b")];
    const result = setDeletedFlags(items, new Set(["a"]), true);
    expect(result[0]).toBe(target);
    expect(result[1]).toBe(items[1]);
  });

  it("clears flags when deleted=false", () => {
    const items = [item("x", { deleted: true })];
    const result = setDeletedFlags(items, new Set(["x"]), false);
    expect(result[0].deleted).toBe(false);
  });
});
