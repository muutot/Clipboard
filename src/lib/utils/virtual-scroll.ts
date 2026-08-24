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

export function trimTrailingBlankLines(text: string | null | undefined): string {
  return (text ?? "").replace(/(?:\r\n?|\n)(?:[ \t]*(?:\r\n?|\n))*[ \t]*$/, "");
}

export function estimateTextLines(text: string | null | undefined, maxLines: number): number {
  const previewText = trimTrailingBlankLines(text);
  if (!previewText) return 0;

  const limit = Math.min(12, Math.max(1, Number.isFinite(maxLines) ? Math.round(maxLines) : 3));
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
}: {
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
}): number {
  const gap = cardGap ?? 5;
  if (kind === "image") return compact ? (compactImage ?? 130) + gap : IMAGE_HEIGHT;

  const visibleLines = showPreview ? Math.max(1, textLines) : 1;
  if (compact) {
    if (visibleLines <= 1) return (compactText ?? 58) + gap;
    return (
      (compactTallText ?? 70) +
      TEXT_LINE_HEIGHT +
      Math.max(0, visibleLines - 2) * TEXT_LINE_HEIGHT +
      gap
    );
  }
  return TEXT_HEIGHT + Math.max(0, visibleLines - 1) * TEXT_LINE_HEIGHT;
}

// --- Canvas-based visual line measurement ---

let _measureCtx: CanvasRenderingContext2D | null = null;

function measureCtx(): CanvasRenderingContext2D | null {
  if (typeof document === "undefined") return null;
  if (!_measureCtx) {
    const canvas = document.createElement("canvas");
    _measureCtx = canvas.getContext("2d");
  }
  return _measureCtx;
}

function systemFontFamily(): string {
  if (typeof document === "undefined") return "system-ui, sans-serif";
  return getComputedStyle(document.documentElement).fontFamily;
}

let _cachedFontFamily: string | null = null;
let _fontFamilyCheckedAt = 0;
function fontFamily(): string {
  // Re-check the computed font family at most once per second so a theme or
  // system font change invalidates stale caches without paying
  // getComputedStyle on every measurement.
  const now = Date.now();
  if (!_cachedFontFamily || now - _fontFamilyCheckedAt > 1000) {
    const computed = systemFontFamily();
    if (_cachedFontFamily && computed !== _cachedFontFamily) {
      _measureCache.clear();
    }
    _cachedFontFamily = computed;
    _fontFamilyCheckedAt = now;
  }
  return _cachedFontFamily;
}

// Memoized visual-line results. The virtual scroll estimator runs this for
// every filtered item whenever layout inputs change; without the cache that
// means O(n) canvas word-measurements per frame during window resizes or
// setting tweaks. Keys are bounded (FIFO eviction) to control memory.
const MEASURE_CACHE_LIMIT = 8192;
const _measureCache = new Map<string, number>();

/**
 * Count visual lines a text string occupies inside a given pixel width,
 * accounting for explicit \\n breaks and word-wrapping.
 */
export function measureVisualLines(
  text: string | null | undefined,
  fontSize: number,
  maxWidth: number,
  maxLines: number,
): number {
  const ctx = measureCtx();
  if (!ctx || !text || maxWidth <= 0 || fontSize <= 0) return 0;

  const limit = Math.min(12, Math.max(1, maxLines));
  const clamped = text.length > 4000 ? text.slice(0, 4000) : text;
  const cacheKey = `${fontSize}|${Math.round(maxWidth)}|${limit}|${clamped}`;
  const cached = _measureCache.get(cacheKey);
  if (cached !== undefined) return cached;

  ctx.font = `${fontSize}px ${fontFamily()}`;

  let totalLines = 0;

  const paragraphs = clamped.split("\n");

  for (const paragraph of paragraphs) {
    if (totalLines >= limit) break;
    if (paragraph.length === 0) {
      totalLines += 1;
      continue;
    }

    // Detect CJK-dominant paragraphs — they can break at any character,
    // so ceil(totalWidth / maxWidth) is both fast and correct.
    let cjkCount = 0;
    for (let i = 0; i < paragraph.length && i < 200; i++) {
      const cp = paragraph.charCodeAt(i);
      if (
        (cp >= 0x4e00 && cp <= 0x9fff) ||
        (cp >= 0x3400 && cp <= 0x4dbf) ||
        (cp >= 0x3000 && cp <= 0x303f) ||
        (cp >= 0xff00 && cp <= 0xffef) ||
        (cp >= 0x3040 && cp <= 0x309f) ||
        (cp >= 0x30a0 && cp <= 0x30ff)
      ) {
        cjkCount += 1;
      }
    }
    const isCjkDominant = paragraph.length > 0 && cjkCount / Math.min(paragraph.length, 200) > 0.3;

    if (isCjkDominant) {
      const fullWidth = ctx.measureText(paragraph).width;
      const visualLines = Math.max(1, Math.ceil(fullWidth / maxWidth));
      totalLines += visualLines;
      if (totalLines > limit) totalLines = limit;
    } else {
      // Word-by-word measurement for Latin text
      const words = paragraph.split(/(?<= )/);
      let lineWidth = 0;
      let paraLines = 1;

      for (const word of words) {
        const wordWidth = ctx.measureText(word).width;
        if (lineWidth + wordWidth > maxWidth && lineWidth > 0) {
          paraLines += 1;
          if (totalLines + paraLines > limit) break;
          lineWidth = wordWidth;
        } else {
          lineWidth += wordWidth;
        }
      }
      totalLines += paraLines;
      if (totalLines > limit) totalLines = limit;
    }
  }

  return rememberMeasurement(cacheKey, Math.max(0, Math.min(limit, totalLines)));
}

function rememberMeasurement(cacheKey: string, lines: number): number {
  if (_measureCache.size >= MEASURE_CACHE_LIMIT) {
    const oldest = _measureCache.keys().next().value;
    if (oldest !== undefined) _measureCache.delete(oldest);
  }
  _measureCache.set(cacheKey, lines);
  return lines;
}

// --- Virtual scrolling ---

export function createVirtualList(
  totalItems: number,
  containerHeight: number,
  scrollTop: number,
  config: VirtualScrollConfig,
  heights?: number[],
  positions?: number[],
): VirtualListResult {
  if (totalItems === 0 || containerHeight <= 0) {
    return { visibleItems: [], totalHeight: 0, offsetY: 0 };
  }

  let pos: number[];
  if (positions && positions.length === totalItems + 1) {
    pos = positions;
  } else {
    pos = [0];
    for (let i = 0; i < totalItems; i++) {
      const h = heights?.[i] ?? config.itemHeight;
      pos.push(pos[i] + h);
    }
  }
  const totalHeight = pos[totalItems];

  // Binary search for the first visible item
  let startIndex = 0;
  let endIndex = totalItems;
  while (startIndex < endIndex) {
    const mid = Math.floor((startIndex + endIndex) / 2);
    if (pos[mid] <= scrollTop) {
      startIndex = mid + 1;
    } else {
      endIndex = mid;
    }
  }
  startIndex = Math.max(0, startIndex - 1 - config.overscan);

  // Find end index based on visible height
  const visibleBottom = scrollTop + containerHeight;
  while (endIndex < totalItems && pos[endIndex] < visibleBottom) {
    endIndex++;
  }
  endIndex = Math.min(totalItems, endIndex + config.overscan);

  const visibleItems: { index: number; top: number }[] = [];
  for (let i = startIndex; i < endIndex; i++) {
    visibleItems.push({ index: i, top: pos[i] });
  }

  const offsetY = pos[startIndex];

  return { visibleItems, totalHeight, offsetY };
}

export function buildPositions(heights: number[], fallbackItemHeight: number): number[] {
  const positions: number[] = [0];
  for (let i = 0; i < heights.length; i++) {
    const h = heights[i] ?? fallbackItemHeight;
    positions.push(positions[i] + h);
  }
  return positions;
}
