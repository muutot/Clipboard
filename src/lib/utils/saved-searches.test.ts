import { describe, expect, it } from "vitest";
import { normalizeSavedSearches, saveNamedSearch, type SavedSearchState } from "./saved-searches";
import { resolveDateRange } from "./history-filter";
import { generalSettings } from "$lib/services/settings";
import { get } from "svelte/store";

const state: SavedSearchState = {
  query: "invoice",
  activeFilter: "image",
  tagFilter: "work",
  sourceAppFilter: "Editor",
  dateFilter: "yesterday",
  sortRules: [{ field: "size", direction: "asc" }],
};

describe("saved searches", () => {
  it("round-trips every axis and resolves relative dates on use", () => {
    const saved = saveNamedSearch([], "Work", state, "one");
    generalSettings.merge(JSON.parse(JSON.stringify({ savedSearches: saved })));
    const restored = get(generalSettings).savedSearches;
    expect(restored).toEqual([{ ...state, name: "Work", id: "one" }]);
    const first = resolveDateRange(restored[0].dateFilter, new Date(2026, 9, 8).getTime())!;
    const next = resolveDateRange(restored[0].dateFilter, new Date(2026, 9, 9).getTime())!;
    expect(next.from).toBeGreaterThan(first.from);
    state.sortRules[0].direction = "desc";
    expect(saved[0].sortRules[0].direction).toBe("asc");
    state.sortRules[0].direction = "asc";
  });
  it("replaces an existing name without losing identity or appending duplicates", () => {
    const saved = saveNamedSearch([], "Work", state, "one");
    expect(saveNamedSearch(saved, " Work ", { ...state, query: "receipt" }, "two")).toEqual([
      { ...state, name: "Work", id: "one", query: "receipt" },
    ]);
  });
  it("bounds and validates persisted data", () => {
    const rows = normalizeSavedSearches([
      null,
      {},
      ...Array.from({ length: 60 }, (_, i) => ({
        id: String(i),
        name: String(i),
        query: "a".repeat(5000),
        activeFilter: "oops",
        dateFilter: 123,
        sortRules: [{ field: "invalid", direction: "asc" }],
      })),
    ]);
    expect(rows).toHaveLength(50);
    expect(rows[0]).toMatchObject({
      query: "a".repeat(4096),
      activeFilter: "all",
      dateFilter: "all",
      sortRules: [{ field: "lastUsedAt", direction: "desc" }],
    });
    expect(normalizeSavedSearches([rows[0], rows[0]])).toHaveLength(1);
  });
});
