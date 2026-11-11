import type { ClipboardItem } from "$lib/types/clipboard";

// Resolves a shift-click range into the ids to select, in the order they should
// be pasted. The multi-selection doubles as the paste queue (a `Set` preserves
// insertion order, see `selection-order.ts`), so the range must follow the
// direction the user selected: click the anchor low in the list, then
// shift-click above it numbers from that anchor downward, rather than always
// numbering the whole range top-to-bottom.
// Kept in one pure function so both directions and the out-of-range anchor are
// covered by unit tests without mounting the main route.

/**
 * Returns the ids between `anchor` and `target` inclusive, ordered from
 * `anchor` toward `target`. An anchor that no longer fits the live list (after
 * a search/filter/removal) is clamped to the nearest valid index; an empty
 * list yields no ids.
 */
export function shiftSelectionIds(
  items: readonly ClipboardItem[],
  anchor: number,
  target: number,
): string[] {
  if (items.length === 0) return [];
  const last = items.length - 1;
  const clampedAnchor = Math.min(Math.max(anchor, 0), last);
  const clampedTarget = Math.min(Math.max(target, 0), last);
  const step = clampedTarget >= clampedAnchor ? 1 : -1;
  const ids: string[] = [];
  // Inclusive of both ends; a downward range walks toward lower indices.
  for (let i = clampedAnchor; ; i += step) {
    ids.push(items[i].id);
    if (i === clampedTarget) break;
  }
  return ids;
}
