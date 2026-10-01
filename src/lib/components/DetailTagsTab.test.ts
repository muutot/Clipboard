import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import DetailTagsTab from "./DetailTagsTab.svelte";

// The add control is the tab's first row and never scrolls away: only the tag
// list scrolls underneath it. That is a DOM-order contract (the panel body
// scrolls as one column, so a trailing input would be pushed out of reach), so
// it is locked here rather than left to CSS inspection.

function item(overrides: Partial<ClipboardItem> = {}): ClipboardItem {
  return {
    id: "entry-1",
    kind: "text",
    title: "title",
    preview: "preview",
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "1 B",
    createdAt: 0,
    favorite: false,
    tags: [],
    ...overrides,
  } as ClipboardItem;
}

const mounted: (() => void)[] = [];
afterEach(() => {
  while (mounted.length) mounted.pop()?.();
});

function render(entry: ClipboardItem) {
  const onsavetags = vi.fn();
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(DetailTagsTab, { target, props: { item: entry, onsavetags } });
  mounted.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  const input = () => target.querySelector<HTMLInputElement>(".tag-input-wrap input")!;
  const rows = () => [...target.querySelectorAll<HTMLElement>(".tag-row")];
  return { target, onsavetags, input, rows };
}

describe("DetailTagsTab", () => {
  it("renders the add input before the tag rows", () => {
    const { target, rows } = render(item({ tags: ["alpha", "beta"] }));

    const tab = target.querySelector(".tags-tab")!;
    const controlIndex = [...tab.children].indexOf(target.querySelector(".tag-input-wrap")!);
    const listIndex = [...tab.children].indexOf(target.querySelector(".tags-list")!);

    expect(controlIndex).toBe(0);
    expect(listIndex).toBeGreaterThan(controlIndex);
    expect(rows()).toHaveLength(2);
  });

  it("keeps the add input in place when there are no tags", () => {
    const { target, input } = render(item());

    expect(target.querySelector(".tags-list")).toBeNull();
    expect(target.querySelector(".tags-empty")).not.toBeNull();
    expect(input()).not.toBeNull();
  });

  it("appends the draft through onsavetags on Enter", () => {
    const { input, onsavetags } = render(item({ tags: ["alpha"] }));

    input().value = " beta ";
    input().dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    flushSync();

    expect(onsavetags).toHaveBeenCalledWith("entry-1", ["alpha", "beta"]);
    expect(input().value).toBe("");
  });

  it("removes a tag through onsavetags", () => {
    const { rows, onsavetags } = render(item({ tags: ["alpha", "beta"] }));

    rows()[0].querySelector<HTMLButtonElement>(".tag-remove")!.click();
    flushSync();

    expect(onsavetags).toHaveBeenCalledWith("entry-1", ["beta"]);
  });
});
