import { estimateCardHeight, type CardEstimateInputs } from "./card-height";
import type { ClipboardItem } from "$lib/types/clipboard";

interface Measurement {
  token: number;
  inputs?: CardEstimateInputs;
  normal?: number;
  hidden?: number;
  editLines?: number;
}
/** The item store replaces records on change. Weak keys neither retain old bodies nor pin evicted rows. */
export function createCardMeasurementCache(estimate = estimateCardHeight) {
  let entries = new WeakMap<ClipboardItem, Measurement>();
  let sequence = 0;
  function entry(item: ClipboardItem): Measurement {
    let cached = entries.get(item);
    if (!cached) {
      cached = { token: ++sequence };
      entries.set(item, cached);
    }
    return cached;
  }
  return {
    // Exact immutable-record identity avoids the old same-length-content signature collisions.
    token(item: ClipboardItem) {
      return entry(item).token;
    },
    estimate(item: ClipboardItem, inputs: CardEstimateInputs, metaHidden: boolean) {
      const cached = entry(item);
      if (cached.inputs !== inputs) {
        cached.inputs = inputs;
        cached.normal = undefined;
        cached.hidden = undefined;
      }
      const key = metaHidden ? "hidden" : "normal";
      return (cached[key] ??= estimate(item, inputs, metaHidden));
    },
    editLines(item: ClipboardItem) {
      const cached = entry(item);
      if (cached.editLines === undefined) {
        let count = 1;
        const text = item.textContent ?? "";
        for (let i = 0; i < text.length; i++) if (text.charCodeAt(i) === 10) count++;
        cached.editLines = count;
      }
      return cached.editLines;
    },
    clear() {
      entries = new WeakMap();
    },
  };
}

/** Preserve derived array identity when measurements did not change, avoiding position rebuilding. */
export function createHeightSequence() {
  let previous: number[] = [];
  return (next: number[]) => {
    if (next.length === previous.length && next.every((value, index) => value === previous[index]))
      return previous;
    previous = next;
    return next;
  };
}
