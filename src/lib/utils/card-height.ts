// Card height estimation extracted from the main route so layout logic can
// evolve (and be tested) independently of component state. The estimator is
// the fallback used when a card has no ResizeObserver measurement yet; keep
// it in sync with the CSS-driven measurements recorded by `recordCardHeight`.

import { itemHeight, measureVisualLines, trimTrailingBlankLines } from "$lib/utils/virtual-scroll";
import { getDisplayRemainingLines } from "$lib/services/clipboard";

export interface CardEstimateInputs {
  compactMode: boolean;
  compactImage: number;
  compactText: number;
  compactTallText: number;
  compactCustomTitle?: number;
  compactCardGap: number;
  compactPaddingTop: number;
  compactPaddingBottom: number;
  showSecondaryText: boolean;
  maxTextLines: number;
  previewFontSize: number;
  contentWidth: number;
}

interface EstimateCandidate {
  kind: string;
  textContent?: string | null;
  preview?: string | null;
  title: string;
  customTitle?: boolean;
}

export function estimateCardHeight(
  item: EstimateCandidate,
  inputs: CardEstimateInputs,
  metaHidden: boolean,
): number {
  const {
    compactMode,
    compactImage,
    compactText,
    compactTallText,
    compactCustomTitle,
    compactCardGap,
    compactPaddingTop,
    compactPaddingBottom,
    showSecondaryText,
    maxTextLines,
    previewFontSize,
    contentWidth,
  } = inputs;

  if (compactMode && item.kind === "image") {
    return (
      compactImage +
      compactPaddingTop +
      compactPaddingBottom +
      4 +
      (metaHidden ? 0 : 14) +
      10 +
      compactCardGap
    );
  }
  if (item.kind !== "text" && item.kind !== "link") {
    return itemHeight({
      kind: item.kind,
      compact: compactMode,
      compactImage,
      compactText,
      compactTallText,
      compactCustomTitle,
      cardGap: compactCardGap,
      showPreview: showSecondaryText,
    });
  }

  let totalLines = 1;
  if (item.customTitle) {
    const bodyLines = showSecondaryText
      ? measureVisualLines(
          trimTrailingBlankLines(item.textContent) || trimTrailingBlankLines(item.preview),
          previewFontSize,
          Math.max(1, contentWidth - 26 - 76),
          maxTextLines,
        )
      : 0;
    totalLines = 1 + bodyLines;
  } else {
    const previewText =
      trimTrailingBlankLines(item.textContent) || trimTrailingBlankLines(item.title);
    const bodyLines = showSecondaryText
      ? measureVisualLines(
          getDisplayRemainingLines(previewText),
          previewFontSize,
          Math.max(1, contentWidth - 26 - 76),
          maxTextLines,
        )
      : 0;
    totalLines = 1 + bodyLines;
  }

  return itemHeight({
    kind: item.kind,
    textLines: totalLines,
    compact: compactMode,
    compactText,
    compactTallText,
    compactImage,
    cardGap: compactCardGap,
    showPreview: showSecondaryText,
  });
}
