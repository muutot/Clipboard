export interface VirtualScrollConfig {
  itemHeight: number;
  overscan: number;
}

export interface VirtualListResult {
  visibleItems: { index: number }[];
  totalHeight: number;
  offsetY: number;
}

export function createVirtualList(
  totalItems: number,
  containerHeight: number,
  scrollTop: number,
  config: VirtualScrollConfig,
): VirtualListResult {
  const { itemHeight, overscan } = config;

  if (totalItems === 0 || containerHeight <= 0) {
    return { visibleItems: [], totalHeight: 0, offsetY: 0 };
  }

  const totalHeight = totalItems * itemHeight;
  const startIndex = Math.max(0, Math.floor(scrollTop / itemHeight) - overscan);
  const visibleCount = Math.ceil(containerHeight / itemHeight) + overscan * 2;
  const endIndex = Math.min(totalItems, startIndex + visibleCount);

  const visibleItems: { index: number }[] = [];
  for (let i = startIndex; i < endIndex; i++) {
    visibleItems.push({ index: i });
  }

  const offsetY = startIndex * itemHeight;

  return { visibleItems, totalHeight, offsetY };
}
