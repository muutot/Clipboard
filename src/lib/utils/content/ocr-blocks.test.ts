import { expect, it } from "vitest";
import { ocrBlockRect, ocrBlockMatches, selectedOcrText } from "./ocr-blocks";
const first = { text: "Invoice 发票", confidence: 0.9, left: 100, top: 50, width: 200, height: 50 };
const second = { ...first, text: "Total", top: 100 };
it("maps original pixels to thumbnail percentages and clamps invalid geometry", () => {
  expect(ocrBlockRect(first, 1000, 500)).toEqual({ left: 10, top: 10, width: 20, height: 10 });
  expect(ocrBlockRect({ ...first, left: 900, width: 300 }, 1000, 500)?.width).toBe(10);
  expect(ocrBlockRect({ ...first, width: NaN }, 1000, 500)).toBeNull();
  expect(ocrBlockRect(first, 0, 500)).toBeNull();
  expect(ocrBlockRect({ ...first, left: 2000 }, 1000, 500)).toBeNull();
});
it("matches Chinese and case-insensitive terms without matching empty queries", () => {
  expect(ocrBlockMatches(first, "invoice")).toBe(true);
  expect(ocrBlockMatches(first, "发票")).toBe(true);
  expect(ocrBlockMatches(first, " ")).toBe(false);
});
it("copies selected blocks in reading order, not click order", () => {
  expect(selectedOcrText([first, second], new Set([1, 0]))).toBe("Invoice 发票\nTotal");
});
