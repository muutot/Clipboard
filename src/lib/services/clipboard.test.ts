import { describe, expect, it } from "vitest";
import { deriveTextEditPatch, generatedClipboardTitle, formatTextLength } from "./clipboard";
import type { ClipboardItem } from "$lib/types/clipboard";

function item(overrides: Partial<ClipboardItem> & { id: string }): ClipboardItem {
  return {
    kind: "text",
    title: "old title",
    preview: "old preview",
    sourceApp: "Notepad",
    sourceTone: "neutral",
    sizeLabel: "0 chars",
    createdAt: 0,
    favorite: false,
    ...overrides,
  };
}

describe("deriveTextEditPatch", () => {
  it("regenerates the title and content for a plain text record", () => {
    const patch = deriveTextEditPatch(item({ id: "a" }), "hello world");
    expect(patch.isText).toBe(true);
    expect(patch.isMedia).toBe(false);
    expect(patch.newTitle).toBe(generatedClipboardTitle("hello world"));
    expect(patch.newTextContent).toBe("hello world");
    expect(patch.newPreview).toBe("old preview");
    expect(patch.newSizeBytes).toBe(11);
    expect(patch.newSizeLabel).toBe(formatTextLength(11));
  });

  it("keeps a custom title but refreshes the text content", () => {
    const patch = deriveTextEditPatch(
      item({ id: "a", customTitle: true, title: "pinned" }),
      "next",
    );
    expect(patch.newTitle).toBe("pinned");
    expect(patch.newTextContent).toBe("next");
  });

  it("treats link records as text", () => {
    const patch = deriveTextEditPatch(item({ id: "a", kind: "link" }), "https://example.com");
    expect(patch.isText).toBe(true);
    expect(patch.newTitle).toBe("https://example.com");
  });

  it("derives the preview from content beyond 200 chars", () => {
    const content = "x".repeat(250);
    const patch = deriveTextEditPatch(item({ id: "a" }), content);
    expect(patch.newPreview).toBe(content.slice(200));
  });

  it("renames media in place without touching the stored text", () => {
    const patch = deriveTextEditPatch(
      item({ id: "a", kind: "image", textContent: "stored" }),
      "new-name.png",
    );
    expect(patch.isMedia).toBe(true);
    expect(patch.isText).toBe(false);
    expect(patch.newTitle).toBe("new-name.png");
    expect(patch.newTextContent).toBe("stored");
    expect(patch.newPreview).toBe("old preview");
  });

  it("nulls the text content for media with no stored text", () => {
    const patch = deriveTextEditPatch(item({ id: "a", kind: "file" }), "report.pdf");
    expect(patch.isMedia).toBe(true);
    expect(patch.newTextContent).toBeNull();
  });
});
