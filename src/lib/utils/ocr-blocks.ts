import type { OcrTextBlock } from "$lib/types/clipboard";

export function ocrBlockMatches(block: OcrTextBlock, query: string): boolean {
  const terms = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const text = block.text.toLocaleLowerCase();
  return terms.some((term) => text.includes(term));
}

/** Clamp malformed/imported geometry to the source image before scaling to percentages. */
export function ocrBlockRect(block: OcrTextBlock, width: number, height: number) {
  if (
    ![width, height, block.left, block.top, block.width, block.height].every(Number.isFinite) ||
    width <= 0 ||
    height <= 0 ||
    block.width <= 0 ||
    block.height <= 0
  )
    return null;
  const left = Math.max(0, Math.min(width, block.left));
  const top = Math.max(0, Math.min(height, block.top));
  const right = Math.max(left, Math.min(width, block.left + block.width));
  const bottom = Math.max(top, Math.min(height, block.top + block.height));
  if (right === left || bottom === top) return null;
  return {
    left: (left / width) * 100,
    top: (top / height) * 100,
    width: ((right - left) / width) * 100,
    height: ((bottom - top) / height) * 100,
  };
}

export function selectedOcrText(blocks: readonly OcrTextBlock[], selected: ReadonlySet<number>) {
  // Engine reading order, independent of the order in which the user clicked.
  return blocks
    .filter((_, index) => selected.has(index))
    .map((block) => block.text)
    .join("\n");
}
