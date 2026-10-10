import type { ClipboardFilter, SortRule } from "$lib/types/clipboard";
import type { HistoryDateFilter } from "../search/history-filter";

export interface SavedSearchState {
  query: string;
  activeFilter: ClipboardFilter;
  tagFilter: string | null;
  sourceAppFilter: string;
  dateFilter: HistoryDateFilter;
  sortRules: SortRule[];
}

export interface SavedSearch extends SavedSearchState {
  id: string;
  name: string;
}

export const MAX_SAVED_SEARCHES = 50;
const groups: ClipboardFilter[] = ["all", "text", "link", "image", "file", "favorite", "deleted"];
const dates: HistoryDateFilter[] = ["all", "today", "yesterday", "week", "month"];
const fields = ["createdAt", "lastUsedAt", "title", "size", "kind", "favorite"];
const bounded = (value: unknown, length: number) =>
  typeof value === "string" ? Array.from(value).slice(0, length).join("") : "";

/** Validate persisted/browser data; dates remain relative until a search is applied. */
export function normalizeSavedSearches(value: unknown): SavedSearch[] {
  if (!Array.isArray(value)) return [];
  const result: SavedSearch[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object") continue;
    const id = bounded(entry.id, 80).trim();
    const name = bounded(entry.name, 80).trim();
    if (!id || !name || result.some((search) => search.id === id || search.name === name)) continue;
    const sortRules: SortRule[] = [];
    if (Array.isArray(entry.sortRules)) {
      for (const rule of entry.sortRules) {
        if (
          rule &&
          fields.includes(rule.field) &&
          (rule.direction === "asc" || rule.direction === "desc") &&
          !sortRules.some((old) => old.field === rule.field)
        ) {
          sortRules.push({ field: rule.field, direction: rule.direction });
        }
      }
    }
    result.push({
      id,
      name,
      query: bounded(entry.query, 4096),
      activeFilter: groups.includes(entry.activeFilter) ? entry.activeFilter : "all",
      dateFilter: dates.includes(entry.dateFilter) ? entry.dateFilter : "all",
      tagFilter: bounded(entry.tagFilter, 256).trim() || null,
      sourceAppFilter: bounded(entry.sourceAppFilter, 512),
      sortRules: sortRules.length ? sortRules : [{ field: "lastUsedAt", direction: "desc" }],
    });
    if (result.length === MAX_SAVED_SEARCHES) break;
  }
  return result;
}

export function saveNamedSearch(
  searches: SavedSearch[],
  name: string,
  state: SavedSearchState,
  id: string = crypto.randomUUID(),
): SavedSearch[] {
  const existing = searches.find((search) => search.name === name.trim());
  const saved = normalizeSavedSearches([{ ...state, id: existing?.id ?? id, name }])[0];
  if (!saved) return searches;
  return normalizeSavedSearches(
    existing
      ? searches.map((search) => (search.id === existing.id ? saved : search))
      : [...searches, saved],
  );
}
