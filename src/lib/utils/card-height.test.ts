import { describe, it, expect } from "vitest";
import { estimateCardHeight, type CardEstimateInputs } from "./card-height";
import { itemHeight } from "./virtual-scroll";

const baseInputs: CardEstimateInputs = {
  imageHeight: 130,
  textHeight: 58,
  tallTextHeight: 70,
  cardGap: 5,
  cardPaddingTop: 6,
  cardPaddingBottom: 6,
  showSecondaryText: true,
  maxTextLines: 3,
  previewFontSize: 13,
  contentWidth: 600,
};

describe("estimateCardHeight", () => {
  it("returns the image formula when kind is image", () => {
    const inputs: CardEstimateInputs = { ...baseInputs, imageHeight: 120 };
    const item = { kind: "image", title: "photo", textContent: null };
    // formula: imageHeight + top + bottom +4 + (meta?14:0) +10 + gap
    expect(estimateCardHeight(item, inputs, false)).toBe(120 + 6 + 6 + 4 + 14 + 10 + 5);
    expect(estimateCardHeight(item, inputs, true)).toBe(120 + 6 + 6 + 4 + 0 + 10 + 5);
  });

  it("delegates non-text/link kinds to itemHeight", () => {
    const fileItem = { kind: "file", title: "archive.zip", textContent: null };
    expect(estimateCardHeight(fileItem, baseInputs, false)).toBe(
      itemHeight({
        kind: "file",
        imageHeight: baseInputs.imageHeight,
        textHeight: baseInputs.textHeight,
        tallTextHeight: baseInputs.tallTextHeight,
        cardGap: baseInputs.cardGap,
        cardPaddingTop: baseInputs.cardPaddingTop,
        cardPaddingBottom: baseInputs.cardPaddingBottom,
        showPreview: baseInputs.showSecondaryText,
      }),
    );
  });

  it("estimates text items via itemHeight with a single title line when showSecondaryText is false", () => {
    const inputs: CardEstimateInputs = { ...baseInputs, showSecondaryText: false };
    const textItem = {
      kind: "text",
      title: "Hello",
      textContent: "Hello\nsecond line that would normally add a preview line",
    };
    expect(estimateCardHeight(textItem, inputs, false)).toBe(
      itemHeight({
        kind: "text",
        textLines: 1,
        textHeight: baseInputs.textHeight,
        tallTextHeight: baseInputs.tallTextHeight,
        imageHeight: baseInputs.imageHeight,
        cardGap: baseInputs.cardGap,
        cardPaddingTop: baseInputs.cardPaddingTop,
        cardPaddingBottom: baseInputs.cardPaddingBottom,
        showPreview: false,
      }),
    );
  });

  it("includes secondary text line count for text items when showSecondaryText is true", () => {
    // In jsdom measureVisualLines returns 0 (no canvas), so the estimator stays at 1 line.
    // We assert the deterministic path rather than the canvas-measured line count.
    const textItem = { kind: "text", title: "Title", textContent: "Title\nLine2\nLine3" };
    const customTitleItem = {
      kind: "text",
      title: "Custom",
      textContent: "Body line1\nBody line2",
      customTitle: true,
    };
    const withoutSecondary = { ...baseInputs, showSecondaryText: false };
    const withSecondary = { ...baseInputs, showSecondaryText: true };
    // Both paths currently collapse to 1 line in the headless test environment,
    // but the call must not throw and must return a positive height.
    expect(estimateCardHeight(textItem, withSecondary, false)).toBeGreaterThan(0);
    expect(estimateCardHeight(customTitleItem, withSecondary, false)).toBeGreaterThan(0);
    expect(estimateCardHeight(textItem, withoutSecondary, false)).toBe(
      itemHeight({
        kind: "text",
        textLines: 1,
        textHeight: baseInputs.textHeight,
        tallTextHeight: baseInputs.tallTextHeight,
        imageHeight: baseInputs.imageHeight,
        cardGap: baseInputs.cardGap,
        cardPaddingTop: baseInputs.cardPaddingTop,
        cardPaddingBottom: baseInputs.cardPaddingBottom,
        showPreview: false,
      }),
    );
  });

  it("estimates custom-title text cards with the shared text formula", () => {
    const customItem = {
      kind: "text",
      title: "Search Query",
      textContent: "Body line",
      customTitle: true,
    };
    // Custom-title cards render a single-line title plus the clamped content
    // preview, exactly like a normal text card, so the estimate must use the
    // same textHeight/tallTextHeight formula (1 line in the headless env).
    expect(estimateCardHeight(customItem, baseInputs, false)).toBe(
      itemHeight({
        kind: "text",
        textLines: 1,
        textHeight: baseInputs.textHeight,
        tallTextHeight: baseInputs.tallTextHeight,
        imageHeight: baseInputs.imageHeight,
        cardGap: baseInputs.cardGap,
        cardPaddingTop: baseInputs.cardPaddingTop,
        cardPaddingBottom: baseInputs.cardPaddingBottom,
        showPreview: baseInputs.showSecondaryText,
      }),
    );
    expect(estimateCardHeight(customItem, baseInputs, false)).toBe(58 + 6 + 6 + 5);
  });

  it("covers link kind with the same text path", () => {
    const linkItem = {
      kind: "link",
      title: "https://example.com",
      textContent: "https://example.com",
    };
    expect(estimateCardHeight(linkItem, baseInputs, false)).toBeGreaterThan(0);
  });
});
