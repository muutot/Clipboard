// Pure bulk-selection planning helper extracted from the main route. It
// contains no IPC, stores, or DOM — the route applies the returned plan
// through its persistence wrappers and the item-store mutators.

import type { ClipboardItem } from "#lib/types/clipboard.ts";

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

/** One merged clipboard payload for a bulk paste: plain text, or an HTML
 * fragment plus its plain-text fallback. Never both forms separately: the
 * rich payload already carries the plain text through the writer. */
export type BulkPastePayload = { text: string } | { html: string; plainText: string };

export interface BulkPastePlan {
  payload: BulkPastePayload;
  /** Ids of the rows that contributed to the merged payload. */
  ids: string[];
}

function escapeHtmlText(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

function plainFragment(text: string): string {
  return `<div>${escapeHtmlText(text).replace(/\n/g, "<br>\n")}</div>`;
}

// A row is rich when it is a text/link row that was captured with HTML.
// Stored `htmlContent` is the CF_HTML fragment (the bytes between the
// StartFragment/EndFragment offsets), so fragments concatenate as siblings
// inside the document shell the backend writer rebuilds around them. Image
// and file rows are never treated as rich: the merged payload has no channel
// for CF_DIB or CF_HDROP, so they fall back to their path text.
function richSource(item: ClipboardItem): string | null {
  if (item.kind === "image" || item.kind === "file") return null;
  const html = item.htmlContent;
  return html && html.trim().length > 0 ? html : null;
}

/**
 * Plans one merged payload for a bulk paste so the whole selection reaches
 * the target through a single clipboard write and a single delivery: with no
 * second write there is no window for the next item's staging to race the
 * previous delivery's asynchronous clipboard read (the item-shift bug).
 * Rich rows keep their HTML fragment; plain text/link rows are escaped into
 * block fragments; image/file rows contribute their path text, matching
 * `planBulkCopy`. Rows are joined with a newline, the same separator as bulk
 * copy. If no row carries HTML the payload stays plain text.
 */
export function planBulkPaste(items: readonly ClipboardItem[]): BulkPastePlan | null {
  const textParts: string[] = [];
  const fragments: (string | null)[] = [];
  const ids: string[] = [];
  for (const item of items) {
    const entry = bulkCopyEntry(item);
    if (entry === null) continue;
    const rich = richSource(item);
    textParts.push(entry);
    fragments.push(rich);
    ids.push(item.id);
  }
  if (ids.length === 0) return null;
  const plainText = textParts.join("\n");
  if (!fragments.some((rich) => rich !== null)) {
    return { payload: { text: plainText }, ids };
  }
  const html = fragments.map((rich, index) => rich ?? plainFragment(textParts[index])).join("\n");
  return { payload: { html, plainText }, ids };
}
