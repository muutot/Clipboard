import { describe, expect, it } from "vitest";

import {
  buildPositions,
  createVirtualList,
  editHeight,
  estimateTextLines,
  itemHeight,
  trimTrailingBlankLines,
} from "./virtual-scroll";

describe("trimTrailingBlankLines", () => {
  it("removes trailing blank lines only", () => {
    expect(trimTrailingBlankLines("a\nb")).toBe("a\nb");
    expect(trimTrailingBlankLines("a\nb\n\n \n")).toBe("a\nb");
    expect(trimTrailingBlankLines("\r\n\r\n")).toBe("");
  });

  it("treats null and undefined as an empty string", () => {
    expect(trimTrailingBlankLines(null)).toBe("");
    expect(trimTrailingBlankLines(undefined)).toBe("");
  });
});

describe("estimateTextLines", () => {
  it("counts logical lines", () => {
    expect(estimateTextLines("one\ntwo", 5)).toBe(2);
  });

  it("clamps to the configured maximum of 12 and minimum of 1", () => {
    const many = Array.from({ length: 40 }, (_, i) => `line${i}`).join("\n");
    expect(estimateTextLines(many, 99)).toBe(12);
    expect(estimateTextLines("only", -3)).toBe(1);
  });

  it("returns zero for blank input", () => {
    expect(estimateTextLines("", 3)).toBe(0);
    // A whitespace-only line still occupies one visual line.
    expect(estimateTextLines("   \n  ", 3)).toBe(1);
  });
});

describe("itemHeight", () => {
  it("uses the compact image height in compact mode", () => {
    expect(itemHeight({ kind: "image", compact: true, compactImage: 130, cardGap: 5 })).toBe(135);
  });

  it("adds one text line height per extra visible line outside compact mode", () => {
    const base = itemHeight({ kind: "text", textLines: 1 });
    expect(base).toBe(88);
    expect(itemHeight({ kind: "text", textLines: 3 })).toBe(88 + 2 * 20);
  });

  it("ignores preview lines when the secondary text is hidden", () => {
    expect(itemHeight({ kind: "text", textLines: 6, showPreview: false })).toBe(
      itemHeight({ kind: "text", textLines: 1 }),
    );
  });
});

describe("editHeight", () => {
  it("clamps the editor between three and twelve rows", () => {
    expect(editHeight(1)).toBe(editHeight(3));
    expect(editHeight(50) - editHeight(12)).toBeLessThan(20);
  });

  it("adds space for the custom title row", () => {
    expect(editHeight(5, true)).toBe(editHeight(5) + 34);
  });

  it("appends the card gap", () => {
    expect(editHeight(5, false, 7)).toBe(editHeight(5) + 7);
  });
});

describe("buildPositions", () => {
  it("returns cumulative offsets with a leading zero", () => {
    expect(buildPositions([10, 20, 30], 999)).toEqual([0, 10, 30, 60]);
  });

  it("falls back to the default height for missing entries", () => {
    const positions = buildPositions([10, undefined as unknown as number], 15);
    expect(positions).toEqual([0, 10, 25]);
  });
});

describe("createVirtualList", () => {
  const heights = Array.from({ length: 100 }, (_, i) => (i % 2 ? 90 : 110));
  const config = { itemHeight: 150, overscan: 1 };

  it("returns an empty result for an empty list or a zero-height viewport", () => {
    expect(createVirtualList(0, 500, 0, config)).toEqual({
      visibleItems: [],
      totalHeight: 0,
      offsetY: 0,
    });
    expect(createVirtualList(10, 0, 0, config).visibleItems).toHaveLength(0);
  });

  it("locates the first visible item via the position index", () => {
    // Cumulative height up to item i is 100*i + (even ? 110 : 90).
    const scrollTop = 100 * 10 + 50;
    const result = createVirtualList(100, 300, scrollTop, config, heights);
    // Binary search lands on the first item past the scroll top, then backs
    // off by one visible item plus the overscan count.
    expect(result.visibleItems[0].index).toBeGreaterThanOrEqual(8);
    expect(result.offsetY).toBeLessThanOrEqual(scrollTop);
  });

  it("includes overscan items on both sides of the viewport", () => {
    const result = createVirtualList(100, 200, 250, config, heights);
    const indexes = result.visibleItems.map((entry) => entry.index);
    expect(Math.min(...indexes)).toBeGreaterThan(0);
    expect(Math.max(...indexes)).toBeLessThan(99);
  });
});
