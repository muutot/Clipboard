import type { ClipboardItem, HistoryFilterArgs } from "$lib/types/clipboard";

/**
 * Client-side search over the browser-preview demo records (the desktop path
 * queries SQLite through `search_clipboard_items`). Mirrors the backend's
 * substring semantics closely enough for preview: case-insensitive match over
 * the searchable text fields, combined with the same history filter args the
 * backend would receive.
 */
export function filterDemoSearchResults(
  items: ClipboardItem[],
  query: string,
  filter: HistoryFilterArgs,
): ClipboardItem[] {
  const needle = query.trim().toLowerCase();
  return items.filter((item) => {
    if (filter.kind && item.kind !== filter.kind) return false;
    if (filter.favorite && !item.favorite) return false;
    if (filter.tag && !(item.tags ?? []).includes(filter.tag)) return false;
    if (filter.sourceApp && item.sourceApp !== filter.sourceApp) return false;
    if (filter.dateFromMs != null && item.createdAt < filter.dateFromMs) return false;
    if (filter.dateToMs != null && item.createdAt > filter.dateToMs) return false;
    if (!needle) return true;
    return [item.title, item.preview, item.searchableText, item.textContent, item.ocrText].some(
      (field) => typeof field === "string" && field.toLowerCase().includes(needle),
    );
  });
}
