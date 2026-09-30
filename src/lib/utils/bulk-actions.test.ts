import { describe, expect, it } from "vitest";
import { planBulkDelete } from "./bulk-actions";
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
