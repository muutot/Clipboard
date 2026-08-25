import { describe, expect, it, vi, afterEach } from "vitest";
import { filterHistoryItems, resolveDateRange, type HistoryFilterState } from "./history-filter";
import type { ClipboardItem } from "$lib/types/clipboard";

function item(overrides: Partial<ClipboardItem> & { id: string }): ClipboardItem {
  return {
    kind: "text",
    title: overrides.id,
    preview: "",
    sourceApp: "Notepad",
    sourceTone: "neutral",
    sizeLabel: "",
    createdAt: Date.parse("2026-08-25T12:00:00"),
    favorite: false,
    ...overrides,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("resolveDateRange", () => {
  it("returns null for the 'all' sentinel and unknown ids", () => {
    expect(resolveDateRange("all")).toBeNull();
    expect(resolveDateRange("unknown")).toBeNull();
  });

  it("bounds today within one local day", () => {
    vi.setSystemTime(new Date("2026-08-25T15:00:00"));
    const range = resolveDateRange("today")!;
    const now = Date.now();
    expect(range.from).toBeLessThanOrEqual(now);
    expect(range.to).toBeGreaterThanOrEqual(now);
  });

  it("resolves yesterday, week, and month windows", () => {
    vi.setSystemTime(new Date("2026-08-25T09:00:00")); // Tuesday
    expect(resolveDateRange("yesterday")!.from).toBe(new Date(2026, 7, 24, 0, 0, 0, 0).getTime());
    expect(resolveDateRange("week")!.from).toBe(new Date(2026, 7, 24).getTime());
    expect(resolveDateRange("month")!.from).toBe(new Date(2026, 7, 1).getTime());
  });
});

describe("filterHistoryItems", () => {
  const base: HistoryFilterState = {
    query: "",
    activeFilter: "all",
    tagFilter: null,
    sourceAppFilter: "",
    dateFilter: "all",
  };

  const live = item({ id: "live" });
  const deleted = item({ id: "gone", deleted: true });
  const favorite = item({ id: "fav", favorite: true });
  const image = item({ id: "img", kind: "image" });

  function run(state: Partial<HistoryFilterState>, items: ClipboardItem[]) {
    return filterHistoryItems({ items }, { ...base, ...state });
  }

  it("routes groups by deletion state", () => {
    const items = [live, deleted, favorite, image];
    expect(run({ activeFilter: "all" }, items).map((entry) => entry.id)).toEqual([
      "live",
      "fav",
      "img",
    ]);
    expect(run({ activeFilter: "deleted" }, items).map((entry) => entry.id)).toEqual(["gone"]);
    expect(run({ activeFilter: "favorite" }, items).map((entry) => entry.id)).toEqual(["fav"]);
    expect(run({ activeFilter: "image" }, items).map((entry) => entry.id)).toEqual(["img"]);
    // Deleted entries never leak into content groups.
    expect(run({ activeFilter: "text" }, items).map((entry) => entry.id)).toEqual(["live", "fav"]);
  });

  it("applies the tag filter as an inclusion rule", () => {
    const tagged = item({ id: "tagged", tags: ["work"] });
    expect(
      run({ activeFilter: "all", tagFilter: "work" }, [live, tagged]).map((e) => e.id),
    ).toEqual(["tagged"]);
  });

  it("matches source apps case-insensitively on a substring", () => {
    const app = item({ id: "app", sourceApp: "Microsoft Edge" });
    expect(run({ activeFilter: "all", sourceAppFilter: "EDGE" }, [app]).map((e) => e.id)).toEqual([
      "app",
    ]);
    expect(run({ activeFilter: "all", sourceAppFilter: "firefox" }, [app])).toEqual([]);
  });

  it("keeps every candidate when indexed results match the exact query", () => {
    const ranked = [item({ id: "hit-2" }), item({ id: "hit-1" })];
    // Indexed candidates bypass keyword re-checking entirely.
    const result = filterHistoryItems(
      { items: [live], indexedItems: ranked, indexedQuery: "note" },
      { ...base, query: "note", activeFilter: "all" },
    );
    expect(result.map((entry) => entry.id)).toEqual(["hit-2", "hit-1"]);
  });

  it("falls back to local keyword matching when the indexed query is stale", () => {
    const alpha = item({ id: "alpha", searchableText: "alpha bravo" });
    const charlie = item({ id: "charlie", searchableText: "charlie delta" });
    const result = filterHistoryItems(
      { items: [alpha, charlie], indexedItems: [alpha], indexedQuery: "stale" },
      { ...base, query: "bravo", activeFilter: "all" },
    );
    expect(result.map((entry) => entry.id)).toEqual(["alpha"]);

    // Multi-keyword queries require every term (unordered AND).
    const both = filterHistoryItems(
      { items: [alpha, charlie] },
      { ...base, query: "ALPHA delta", activeFilter: "all" },
    );
    expect(both).toEqual([]);
    const single = filterHistoryItems(
      { items: [alpha, charlie] },
      { ...base, query: "delta", activeFilter: "all" },
    );
    expect(single.map((entry) => entry.id)).toEqual(["charlie"]);
  });

  it("treats a natural-language date token as a filter rather than a keyword", () => {
    vi.setSystemTime(new Date("2026-08-25T12:00:00"));
    const capturedYesterday = item({
      id: "captured-yesterday",
      createdAt: new Date(2026, 7, 24, 9).getTime(),
      searchableText: "daily standup notes",
    });
    const capturedToday = item({
      id: "captured-today",
      createdAt: new Date(2026, 7, 25, 9).getTime(),
      searchableText: "standup notes",
    });

    const result = filterHistoryItems(
      { items: [capturedYesterday, capturedToday] },
      { ...base, query: "昨天", activeFilter: "all" },
    );
    // "昨天" narrows the window to yesterday; the record text itself never
    // needs to contain the token.
    expect(result.map((entry) => entry.id)).toEqual(["captured-yesterday"]);
  });

  it("combines group, date, and keyword axes in one pass", () => {
    vi.setSystemTime(new Date("2026-08-25T12:00:00"));
    const inRange = item({
      id: "in-range",
      createdAt: new Date(2026, 7, 20).getTime(),
      searchableText: "invoice scan",
      favorite: true,
    });
    const wrongKind = item({
      id: "wrong-kind",
      kind: "link",
      createdAt: new Date(2026, 7, 20).getTime(),
      searchableText: "invoice link",
    });
    const outOfRange = item({
      id: "out-of-range",
      createdAt: new Date(2024, 5, 1).getTime(),
      searchableText: "invoice old",
    });

    const result = run({ activeFilter: "favorite", dateFilter: "month", query: "invoice" }, [
      inRange,
      wrongKind,
      outOfRange,
    ]);
    expect(result.map((entry) => entry.id)).toEqual(["in-range"]);
  });
});
