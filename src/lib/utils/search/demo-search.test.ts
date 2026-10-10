import { describe, expect, it } from "vitest";
import { filterDemoSearchResults } from "./demo-search";
import type { ClipboardItem } from "$lib/types/clipboard";

function item(overrides: Partial<ClipboardItem>): ClipboardItem {
  return {
    id: "demo",
    kind: "text",
    title: "Tauri 2 Documentation",
    preview: "https://v2.tauri.app/",
    sourceApp: "Browser",
    sourceTone: "blue",
    sizeLabel: "21 个字符",
    createdAt: 1_000,
    favorite: false,
    ...overrides,
  };
}

describe("filterDemoSearchResults", () => {
  const items = [
    item({ id: "a" }),
    item({ id: "b", title: "待办笔记", preview: "", searchableText: "整理 Tauri 键位" }),
    item({ id: "c", kind: "image", title: "截图", preview: "", sourceApp: "SiYuan" }),
    item({ id: "d", favorite: true, title: "favorite tauri link" }),
  ];

  it("matches case-insensitively across searchable fields", () => {
    const results = filterDemoSearchResults(items, "tauri", {});
    expect(results.map((r) => r.id)).toEqual(["a", "b", "d"]);
  });

  it("applies kind and favorite filters together with the query", () => {
    expect(filterDemoSearchResults(items, "", { kind: "image" }).map((r) => r.id)).toEqual(["c"]);
    expect(filterDemoSearchResults(items, "tauri", { favorite: true }).map((r) => r.id)).toEqual([
      "d",
    ]);
    expect(filterDemoSearchResults(items, "", { favorite: true }).map((r) => r.id)).toEqual(["d"]);
  });

  it("applies source app, tag, and date-range filters", () => {
    expect(filterDemoSearchResults(items, "", { sourceApp: "SiYuan" }).map((r) => r.id)).toEqual([
      "c",
    ]);
    const tagged = [...items, item({ id: "e", title: "work", tags: ["work"] })];
    expect(filterDemoSearchResults(tagged, "", { tag: "work" }).map((r) => r.id)).toEqual(["e"]);
    expect(
      filterDemoSearchResults(items, "", { dateFromMs: 2_000, dateToMs: 3_000 }).map((r) => r.id),
    ).toEqual([]);
    expect(
      filterDemoSearchResults(items, "", { dateFromMs: 500, dateToMs: 1_500 }).map((r) => r.id),
    ).toEqual(["a", "b", "c", "d"]);
  });

  it("returns everything for a blank query with no filters", () => {
    expect(filterDemoSearchResults(items, "   ", {})).toHaveLength(4);
  });
});
