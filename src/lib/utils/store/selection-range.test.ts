import { describe, expect, it } from "vitest";
import type { ClipboardItem } from "#lib/types/clipboard.ts";
import { shiftSelectionIds } from "./selection-range";

// The ids double as the paste queue, so order (not just membership) is the
// behavior under test. These mirror the main-route selectItem branch that used
// to always walk from the lower index upward.
function items(ids: string[]): ClipboardItem[] {
  return ids.map((id) => ({ id }) as ClipboardItem);
}

describe("shift selection range", () => {
  const list = items(["one", "two", "three", "four", "five"]);

  it("numbers a downward range from the lower anchor to the target", () => {
    expect(shiftSelectionIds(list, 0, 3)).toEqual(["one", "two", "three", "four"]);
  });

  it("numbers an upward range from the anchor downward to the target", () => {
    expect(shiftSelectionIds(list, 4, 1)).toEqual(["five", "four", "three", "two"]);
  });

  it("keeps the anchor inclusive when both ends are the same", () => {
    expect(shiftSelectionIds(list, 2, 2)).toEqual(["three"]);
  });

  it("clamps an anchor older than the live list instead of reading out of bounds", () => {
    expect(shiftSelectionIds(list, 99, 2)).toEqual(["five", "four", "three"]);
  });

  it("returns no ids for an empty list", () => {
    expect(shiftSelectionIds([], 0, 3)).toEqual([]);
  });
});
