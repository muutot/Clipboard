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
const TALL_TEXT_HEIGHT = 88;
const IMAGE_HEIGHT = 150;

export function editHeight(lineCount: number, hasCustomTitle?: boolean): number {
  const rows = Math.min(12, Math.max(3, lineCount));
  let h = 25 + 8 + (rows * 20) + 36;
  if (hasCustomTitle) h += 34;
  return h;
}

export function itemHeight(
  kind: string,
  hasPreview?: boolean,
  compact?: boolean,
  compactText?: number,
  compactTallText?: number,
  compactImage?: number,
  cardGap?: number,
  showPreview?: boolean,
  customTitle?: boolean,
  compactCustomTitle?: number,
): number {
  if (customTitle && compact) return (compactCustomTitle ?? 80) + (cardGap ?? 5);
  const effectivePreview = hasPreview && showPreview !== false;
  if (kind === "image") return compact ? (compactImage ?? 130) : IMAGE_HEIGHT;
  if (compact) return (effectivePreview ? (compactTallText ?? 70) : (compactText ?? 58)) + (cardGap ?? 5);
  return effectivePreview ? TALL_TEXT_HEIGHT : TEXT_HEIGHT;
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
