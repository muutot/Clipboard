import { expect, it, vi } from "vitest";
import { createCardMeasurementCache, createHeightSequence } from "./card-measurements";
import type { ClipboardItem } from "$lib/types/clipboard";
import type { CardEstimateInputs } from "./card-height";
const item = { id: "a", kind: "text", title: "aaaa", textContent: "aaaa\nbbbb" } as ClipboardItem;
const inputs = { textHeight: 60 } as CardEstimateInputs;
it("reuses estimates per immutable item and mode, invalidating on layout or content changes", () => {
  const measure = vi.fn(() => 70),
    cache = createCardMeasurementCache(measure);
  expect(cache.estimate(item, inputs, false)).toBe(70);
  cache.estimate(item, inputs, false);
  expect(measure).toHaveBeenCalledTimes(1);
  cache.estimate(item, inputs, true);
  expect(measure).toHaveBeenCalledTimes(2);
  cache.estimate(item, { ...inputs }, false);
  expect(measure).toHaveBeenCalledTimes(3);
  const edited = { ...item, textContent: "WWWW\nWWWW" };
  expect(cache.token(edited)).not.toBe(cache.token(item));
  cache.estimate(edited, inputs, false);
  expect(measure).toHaveBeenCalledTimes(4);
  expect(cache.editLines(item)).toBe(2);
  const token = cache.token(item);
  cache.clear();
  expect(cache.token(item)).not.toBe(token);
});
it("reuses unchanged height sequences but updates changed geometry", () => {
  const stable = createHeightSequence(),
    first = stable([10, 20]);
  expect(stable([10, 20])).toBe(first);
  expect(stable([10, 21])).not.toBe(first);
});
