// Search history and suggestion helpers extracted from the main route so
// history normalization, storage, and suggestion slicing can be tested in
// isolation. The route remains the only place that touches
// `window.localStorage` and `$generalSettings`.

export const SEARCH_HISTORY_STORAGE_KEY = "clipboard.search-history.v1";
export const SEARCH_HISTORY_LIMIT = 8;
export const SEARCH_TERM_MAX_LENGTH = 120;
export const SEARCH_SUGGESTION_LIMIT = 8;

export function normalizeSearchTerm(value: string): string {
  return value.trim().slice(0, SEARCH_TERM_MAX_LENGTH);
}

export function loadSearchHistory(storage: Pick<Storage, "getItem"> | null): string[] {
  try {
    const raw = storage?.getItem(SEARCH_HISTORY_STORAGE_KEY) ?? null;
    if (!raw) return [];
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];

    const seen = new Set<string>();
    const result: string[] = [];
    for (const value of parsed) {
      if (typeof value !== "string") continue;
      const term = normalizeSearchTerm(value);
      const key = term.toLocaleLowerCase();
      if (!term || seen.has(key)) continue;
      seen.add(key);
      result.push(term);
      if (result.length >= SEARCH_HISTORY_LIMIT) break;
    }
    return result;
  } catch {
    return [];
  }
}

export function persistSearchHistory(
  storage: Pick<Storage, "setItem"> | null,
  history: string[],
): void {
  try {
    storage?.setItem(
      SEARCH_HISTORY_STORAGE_KEY,
      JSON.stringify(history.slice(0, SEARCH_HISTORY_LIMIT)),
    );
  } catch {
    // Browser privacy settings and desktop webview policies may disable
    // localStorage. Search history remains available for this session.
  }
}

export function nextSearchHistory(history: string[], term: string): string[] {
  const normalized = normalizeSearchTerm(term);
  if (!normalized) return history;
  const key = normalized.toLocaleLowerCase();
  const next = [normalized, ...history.filter((entry) => entry.toLocaleLowerCase() !== key)].slice(
    0,
    SEARCH_HISTORY_LIMIT,
  );
  return next;
}

export function suggestionCandidate(
  value: string | null | undefined,
  queryValue = "",
  alignToQuery = false,
): string | null {
  if (!value) return null;
  let candidate = value.replace(/\s+/g, " ").trim();
  const normalizedQuery = queryValue.toLocaleLowerCase();
  const matchIndex = normalizedQuery ? candidate.toLocaleLowerCase().indexOf(normalizedQuery) : -1;
  if (normalizedQuery && matchIndex < 0) return null;
  if (matchIndex > 0 && (alignToQuery || candidate.length > SEARCH_TERM_MAX_LENGTH)) {
    candidate = candidate.slice(matchIndex);
  }
  candidate = candidate.slice(0, SEARCH_TERM_MAX_LENGTH).trim();
  return candidate.length >= 2 ? candidate : null;
}
