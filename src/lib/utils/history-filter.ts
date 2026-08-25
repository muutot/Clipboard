// History filtering extracted from the main route so the group/date/source/
// keyword rules (and their indexed-result bypass) can be tested in isolation.
// The route only supplies state; this module never touches stores or IPC.

import { parseDateQuery, startOfDay, endOfDay, startOfWeek } from "$lib/utils/date-query";
import type { ClipboardFilter, ClipboardItem } from "$lib/types/clipboard";

export type HistoryDateFilter = "all" | "today" | "yesterday" | "week" | "month";

export interface DateRange {
  from: number;
  to: number;
}

export function resolveDateRange(
  filter: HistoryDateFilter | string,
  now = Date.now(),
): DateRange | null {
  const dayMs = 24 * 60 * 60 * 1_000;

  switch (filter) {
    case "today":
      return { from: startOfDay(now), to: endOfDay(now) };
    case "yesterday":
      return { from: startOfDay(now - dayMs), to: endOfDay(now - dayMs) };
    case "week":
      return { from: startOfWeek(now), to: endOfDay(now) };
    case "month": {
      const first = new Date(now);
      first.setDate(1);
      first.setHours(0, 0, 0, 0);
      return { from: first.getTime(), to: endOfDay(now) };
    }
    default:
      return null;
  }
}

export interface HistoryFilterState {
  query: string;
  activeFilter: ClipboardFilter;
  /** Selected registry tag, or null when no tag filter is active. */
  tagFilter: string | null;
  sourceAppFilter: string;
  dateFilter: HistoryDateFilter | string;
}

interface FilterCandidates {
  items: ClipboardItem[];
  /** Backend search results for `indexedQuery`, or null when unavailable. */
  indexedItems: ClipboardItem[] | null;
  indexedQuery: string;
}

function matchesGroup(item: ClipboardItem, activeFilter: ClipboardFilter): boolean {
  const isDeleted = !!item.deleted;
  if (activeFilter === "all") return !isDeleted;
  if (activeFilter === "deleted") return isDeleted;
  if (activeFilter === "favorite") return !isDeleted && item.favorite;
  return !isDeleted && item.kind === activeFilter;
}

/**
 * Applies the main-window history filters. When backend search results are
 * available for the exact current query they replace the local candidate list
 * and keyword re-checking is skipped — Tantivy already ranked them.
 */
export function filterHistoryItems(
  candidates: Pick<FilterCandidates, "items"> &
    Partial<Pick<FilterCandidates, "indexedItems" | "indexedQuery">>,
  state: HistoryFilterState,
): ClipboardItem[] {
  const normalizedQuery = state.query.trim();
  const usesIndexedResults =
    state.activeFilter !== "deleted" &&
    candidates.indexedItems !== null &&
    candidates.indexedQuery === normalizedQuery;
  const items = usesIndexedResults ? (candidates.indexedItems ?? []) : candidates.items;

  const dateRange = resolveDateRange(state.dateFilter);
  const dateRangeFromNl = !dateRange ? parseDateQuery(normalizedQuery) : null;
  const effectiveDateRange = dateRange ?? dateRangeFromNl;
  // A natural-language date token is a filter, not content that must occur
  // in the record text.
  const keywords = dateRangeFromNl
    ? []
    : normalizedQuery.toLocaleLowerCase().split(/\s+/).filter(Boolean);

  return items.filter((item) => {
    if (!matchesGroup(item, state.activeFilter)) return false;

    if (state.tagFilter && !(item.tags ?? []).includes(state.tagFilter)) return false;

    if (
      state.sourceAppFilter &&
      !item.sourceApp.toLowerCase().includes(state.sourceAppFilter.toLowerCase())
    ) {
      return false;
    }

    if (effectiveDateRange) {
      if (item.createdAt < effectiveDateRange.from || item.createdAt > effectiveDateRange.to) {
        return false;
      }
    }

    if (keywords.length === 0 || usesIndexedResults) {
      return true;
    }

    return keywords.every((keyword) => (item.searchableText ?? "").includes(keyword));
  });
}
