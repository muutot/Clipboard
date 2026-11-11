import { describe, expect, it } from "vitest";
import { planBulkCopy, planBulkDelete, planBulkPaste } from "./bulk-actions";
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

describe("planBulkPaste", () => {
  it("returns a plain-text payload joined with newlines when nothing is rich", () => {
    const selection = [
      item("a", { kind: "text", textContent: "one" }),
      item("b", { kind: "link", title: "Docs", textContent: "https://example.com" }),
    ];
    expect(planBulkPaste(selection)).toEqual({
      payload: { text: "one\nhttps://example.com" },
      ids: ["a", "b"],
    });
  });

  it("merges image and file rows into their path text", () => {
    const selection = [
      item("note", { kind: "text", textContent: "keep" }),
      item("image", { kind: "image", title: "shot", resourcePath: "C:\\pic.png" }),
      item("files", {
        kind: "file",
        textContent: JSON.stringify(["C:\\a.txt", "C:\\b.txt"]),
      }),
    ];
    const plan = planBulkPaste(selection);
    expect(plan?.payload).toEqual({ text: "keep\nC:\\pic.png\nC:\\a.txt\nC:\\b.txt" });
    expect(plan?.ids).toEqual(["note", "image", "files"]);
  });

  it("concatenates HTML fragments and escapes plain rows when any item is rich", () => {
    const selection = [
      item("rich", { kind: "text", textContent: "bold", htmlContent: "<b>bold</b>" }),
      item("plain", { kind: "text", textContent: 'a<b>"quote"' }),
    ];
    const plan = planBulkPaste(selection);
    expect(plan?.ids).toEqual(["rich", "plain"]);
    const payload = plan?.payload as { html: string; plainText: string };
    expect(payload.plainText).toBe('bold\na<b>"quote"');
    // The rich fragment passes through verbatim; the plain row is escaped
    // into a block so the merged fragment stays well-formed markup.
    expect(payload.html).toContain("<b>bold</b>");
    expect(payload.html).toContain("a&lt;b&gt;&quot;quote&quot;");
    expect(payload.html.indexOf("<b>bold</b>")).toBeLessThan(payload.html.indexOf("a&lt;b&gt;"));
  });

  it("keeps media rows as path blocks inside a mixed rich payload", () => {
    const selection = [
      item("rich", { kind: "text", textContent: "bold", htmlContent: "<p>bold</p>" }),
      item("image", { kind: "image", title: "shot", resourcePath: "C:\\pic.png" }),
    ];
    const payload = planBulkPaste(selection)?.payload as { html: string; plainText: string };
    expect(payload.plainText).toBe("bold\nC:\\pic.png");
    expect(payload.html).toContain("<p>bold</p>");
    expect(payload.html).toContain("<div>C:\\pic.png</div>");
  });

  it("returns null when no row contributes content", () => {
    expect(planBulkPaste([item("image", { kind: "image", title: "remote" })])).toBeNull();
    expect(planBulkPaste([])).toBeNull();
  });
});
