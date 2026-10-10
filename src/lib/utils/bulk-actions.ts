// Pure bulk-selection planning helper extracted from the main route. It
// contains no IPC, stores, or DOM — the route applies the returned plan
// through its persistence wrappers and the item-store mutators.

import type { ClipboardItem } from "$lib/types/clipboard";

export interface BulkDeletePlan {
  /** Recycle-bin candidates: soft delete + keep in list as deleted. */
  softIds: string[];
  /** Already-deleted rows: permanent delete. */
  permanentIds: string[];
  /** Active rows with recycle bin off (or favorites excluded): hard delete. */
  hardIds: string[];
}

/**
 * Splits a selection into soft/permanent/hard delete buckets.
 * Favorites are never hard-deleted by capacity-style rules here; an active
 * favorite is simply left out of the plan (matching route behavior where the
 * UI prevents favoriting into deletion).
 */
export function planBulkDelete(
  selectedItems: ClipboardItem[],
  useRecycleBin: boolean,
): BulkDeletePlan {
  const softIds: string[] = [];
  const permanentIds: string[] = [];
  const hardIds: string[] = [];
  for (const item of selectedItems) {
    if (item.deleted) {
      permanentIds.push(item.id);
    } else if (!item.favorite) {
      (useRecycleBin ? softIds : hardIds).push(item.id);
    }
  }
  return { softIds, permanentIds, hardIds };
}

export interface BulkCopyPayload {
  /** Newline-joined plain text written to the clipboard. */
  text: string;
  /** Selected rows that actually contributed content. */
  copiedCount: number;
}

/** Parses a file record's MULTI-file path list (stored as JSON text). */
function filePathList(textContent: string | null | undefined): string[] | null {
  if (!textContent || !textContent.startsWith("[")) return null;
  try {
    const parsed = JSON.parse(textContent) as unknown;
    if (!Array.isArray(parsed)) return null;
    const paths = parsed.filter(
      (entry): entry is string => typeof entry === "string" && entry.trim().length > 0,
    );
    return paths.length > 0 ? paths : null;
  } catch {
    return null;
  }
}

function bulkCopyEntry(item: ClipboardItem): string | null {
  if (item.kind === "text" || item.kind === "link") {
    const text = item.textContent || item.title;
    return text.length > 0 ? text : null;
  }
  if (item.kind === "file") {
    const paths = filePathList(item.textContent);
    if (paths) return paths.join("\n");
  }
  return item.resourcePath ? item.resourcePath : null;
}

/**
 * Plans the plain-text payload for a bulk copy. Text/link rows contribute
 * their full content; image/file rows contribute their source path (or the
 * multi-file path list), the same representation the single-item copy uses,
 * instead of the display title -- a title is not clipboard content, so the
 * old behavior silently reduced media rows to their names. Rows with nothing
 * copyable are dropped so `copiedCount` stays honest.
 */
export function planBulkCopy(items: readonly ClipboardItem[]): BulkCopyPayload {
  const parts: string[] = [];
  for (const item of items) {
    const entry = bulkCopyEntry(item);
    if (entry !== null) parts.push(entry);
  }
  return { text: parts.join("\n"), copiedCount: parts.length };
}
