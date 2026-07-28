export interface VirtualScrollConfig {
  itemHeight: number;
  overscan: number;
}

export interface VirtualListResult {
  visibleItems: { index: number; top: number }[];
  totalHeight: number;
  offsetY: number;
}

const TEXT_HEIGHT = 88;
const IMAGE_HEIGHT = 150;
export const TEXT_LINE_HEIGHT = 20;
export const PREVIEW_LINE_HEIGHT = 16;

export interface ItemHeightOptions {
  kind: string;
  textLines?: number;
  compact?: boolean;
  compactText?: number;
  compactTallText?: number;
  compactImage?: number;
  cardGap?: number;
  showPreview?: boolean;
  customTitle?: boolean;
  compactCustomTitle?: number;
}

export function trimTrailingBlankLines(text: string | null | undefined): string {
  return (text ?? "").replace(/(?:\r\n?|\n)(?:[ \t]*(?:\r\n?|\n))*[ \t]*$/, "");
}

export function estimateTextLines(text: string | null | undefined, maxLines: number): number {
  const previewText = trimTrailingBlankLines(text);
  if (!previewText) return 0;

  const limit = Math.min(12, Math.max(1, Number.isFinite(maxLines) ? Math.round(maxLines) : 3));
  // Visual wrapping depends on the live card width and is corrected from the
  // card's ResizeObserver measurement.  Only explicit newlines are stable
  // enough to use for the initial virtual-scroll estimate.
  return Math.min(limit, previewText.replace(/\r\n?/g, "\n").split("\n").length);
}

export function editHeight(lineCount: number, hasCustomTitle?: boolean, cardGap?: number): number {
  const rows = Math.min(12, Math.max(3, lineCount));
  let h = 25 + 8 + rows * 20 + 36;
  if (hasCustomTitle) h += 34;
  return h + (cardGap ?? 0);
}

export function itemHeight({
  kind,
  textLines = 1,
  compact = false,
  compactText,
  compactTallText,
  compactImage,
  cardGap,
  showPreview = true,
  customTitle = false,
  compactCustomTitle,
}: ItemHeightOptions): number {
  const gap = cardGap ?? 5;
  if (kind === "image") return compact ? (compactImage ?? 130) + gap : IMAGE_HEIGHT;

  const visibleLines = showPreview ? Math.max(1, textLines) : 1;
  if (compact) {
    if (customTitle) {
      const baseHeight = visibleLines > 1 ? (compactCustomTitle ?? 80) : (compactText ?? 58);
      return baseHeight + Math.max(0, visibleLines - 2) * TEXT_LINE_HEIGHT + gap;
    }
    const baseHeight = visibleLines > 1 ? (compactTallText ?? 70) : (compactText ?? 58);
    return baseHeight + Math.max(0, visibleLines - 2) * TEXT_LINE_HEIGHT + gap;
  }
  return TEXT_HEIGHT + Math.max(0, visibleLines - 1) * TEXT_LINE_HEIGHT;
}

export function createVirtualList(
  totalItems: number,
  containerHeight: number,
  scrollTop: number,
  config: VirtualScrollConfig,
  heights?: number[],
): VirtualListResult {
  if (totalItems === 0 || containerHeight <= 0) {
    return { visibleItems: [], totalHeight: 0, offsetY: 0 };
  }

  // Build cumulative position array from heights
  const positions: number[] = [0];
  for (let i = 0; i < totalItems; i++) {
    const h = heights?.[i] ?? config.itemHeight;
    positions.push(positions[i] + h);
  }
  const totalHeight = positions[totalItems];

  // Binary search for the first visible item
  let startIndex = 0;
  let endIndex = totalItems;
  while (startIndex < endIndex) {
    const mid = Math.floor((startIndex + endIndex) / 2);
    if (positions[mid] <= scrollTop) {
      startIndex = mid + 1;
    } else {
      endIndex = mid;
    }
  }
  startIndex = Math.max(0, startIndex - 1 - config.overscan);

  // Find end index based on visible height
  const visibleBottom = scrollTop + containerHeight;
  while (endIndex < totalItems && positions[endIndex] < visibleBottom) {
    endIndex++;
  }
  endIndex = Math.min(totalItems, endIndex + config.overscan);

  const visibleItems: { index: number; top: number }[] = [];
  for (let i = startIndex; i < endIndex; i++) {
    visibleItems.push({ index: i, top: positions[i] });
  }

  const offsetY = positions[startIndex];

  return { visibleItems, totalHeight, offsetY };
}
