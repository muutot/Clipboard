import { describe, expect, it } from "vitest";
import { planBulkCopy, planBulkDelete } from "./bulk-actions";
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

describe("planBulkCopy", () => {
  it("joins the full text of text and link rows", () => {
    const selection = [
      item("a", { kind: "text", textContent: "first line\nsecond" }),
      item("b", { kind: "link", title: "Docs", textContent: "https://example.com" }),
    ];
    expect(planBulkCopy(selection)).toEqual({
      text: "first line\nsecond\nhttps://example.com",
      copiedCount: 2,
    });
  });

  it("copies the stored path list for multi-file rows", () => {
    const selection = [
      item("files", {
        kind: "file",
        textContent: JSON.stringify(["C:\\a.txt", "C:\\b.txt"]),
        resourcePath: "C:\\a.txt",
      }),
    ];
    expect(planBulkCopy(selection)).toEqual({
      text: "C:\\a.txt\nC:\\b.txt",
      copiedCount: 1,
    });
  });

  it("copies the resource path for image and single-file rows", () => {
    const selection = [
      item("image", { kind: "image", title: "screenshot", resourcePath: "C:\\pic.png" }),
      item("file", { kind: "file", title: "clip.mp4", resourcePath: "C:\\clip.mp4" }),
    ];
    expect(planBulkCopy(selection)).toEqual({
      text: "C:\\pic.png\nC:\\clip.mp4",
      copiedCount: 2,
    });
  });

  it("drops rows with nothing copyable instead of falling back to their titles", () => {
    const selection = [
      item("remote-image", { kind: "image", title: "remote" }),
      item("note", { kind: "text", textContent: "keep" }),
    ];
    expect(planBulkCopy(selection)).toEqual({ text: "keep", copiedCount: 1 });
  });

  it("reports an empty payload for a media-only selection with no paths", () => {
    expect(planBulkCopy([item("image", { kind: "image", title: "remote" })])).toEqual({
      text: "",
      copiedCount: 0,
    });
  });
});
