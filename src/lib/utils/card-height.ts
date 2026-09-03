// Card height estimation extracted from the main route so layout logic can
// evolve (and be tested) independently of component state. The estimator is
// the fallback used when a card has no ResizeObserver measurement yet; keep
// it in sync with the CSS-driven measurements recorded by `recordCardHeight`.
// Every kind shares one contract: the *-height settings are content heights
// and cardPaddingTop/Bottom expand the estimated card height externally.

import { itemHeight, measureVisualLines, trimTrailingBlankLines } from "$lib/utils/virtual-scroll";
import { getDisplayRemainingLines } from "$lib/services/clipboard";

export interface CardEstimateInputs {
  imageHeight: number;
  textHeight: number;
  tallTextHeight: number;
  customTitleHeight?: number;
  cardGap: number;
  cardPaddingTop: number;
  cardPaddingBottom: number;
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
    imageHeight,
    textHeight,
    tallTextHeight,
    customTitleHeight,
    cardGap,
    cardPaddingTop,
    cardPaddingBottom,
    showSecondaryText,
    maxTextLines,
    previewFontSize,
    contentWidth,
  } = inputs;

  if (item.kind === "image") {
    return (
      imageHeight + cardPaddingTop + cardPaddingBottom + 4 + (metaHidden ? 0 : 14) + 10 + cardGap
    );
  }
  if (item.kind !== "text" && item.kind !== "link") {
    return itemHeight({
      kind: item.kind,
      imageHeight,
      textHeight,
      tallTextHeight,
      customTitleHeight,
      cardGap,
      cardPaddingTop,
      cardPaddingBottom,
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
    textHeight,
    tallTextHeight,
    imageHeight,
    cardGap: cardGap,
    cardPaddingTop,
    cardPaddingBottom,
    showPreview: showSecondaryText,
  });
}
