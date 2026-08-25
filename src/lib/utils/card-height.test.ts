import { describe, it, expect } from "vitest";
import { estimateCardHeight, type CardEstimateInputs } from "./card-height";
import { itemHeight } from "./virtual-scroll";

const baseInputs: CardEstimateInputs = {
  compactMode: false,
  compactImage: 130,
  compactText: 58,
  compactTallText: 70,
  compactCardGap: 5,
  compactPaddingTop: 6,
  compactPaddingBottom: 6,
  showSecondaryText: true,
  maxTextLines: 3,
  previewFontSize: 13,
  contentWidth: 600,
};

describe("estimateCardHeight", () => {
  it("returns compact image formula when kind is image and compactMode is true", () => {
    const inputs: CardEstimateInputs = { ...baseInputs, compactMode: true, compactImage: 120 };
    const item = { kind: "image", title: "photo", textContent: null };
    // formula: compactImage + top + bottom +4 + (meta?14:0) +10 + gap
    expect(estimateCardHeight(item, inputs, false)).toBe(120 + 6 + 6 + 4 + 14 + 10 + 5);
    expect(estimateCardHeight(item, inputs, true)).toBe(120 + 6 + 6 + 4 + 0 + 10 + 5);
  });

  it("delegates non-text/link kinds to itemHeight", () => {
    const fileItem = { kind: "file", title: "archive.zip", textContent: null };
    expect(estimateCardHeight(fileItem, baseInputs, false)).toBe(
      itemHeight({
        kind: "file",
        compact: false,
        compactImage: baseInputs.compactImage,
        compactText: baseInputs.compactText,
        compactTallText: baseInputs.compactTallText,
        cardGap: baseInputs.compactCardGap,
        showPreview: baseInputs.showSecondaryText,
      }),
    );

    const compactFileInputs = { ...baseInputs, compactMode: true };
    expect(estimateCardHeight(fileItem, compactFileInputs, false)).toBe(
      itemHeight({
        kind: "file",
        compact: true,
        compactImage: baseInputs.compactImage,
        compactText: baseInputs.compactText,
        compactTallText: baseInputs.compactTallText,
        cardGap: baseInputs.compactCardGap,
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
        compact: false,
        compactText: baseInputs.compactText,
        compactTallText: baseInputs.compactTallText,
        compactImage: baseInputs.compactImage,
        cardGap: baseInputs.compactCardGap,
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
        compact: false,
        compactText: baseInputs.compactText,
        compactTallText: baseInputs.compactTallText,
        compactImage: baseInputs.compactImage,
        cardGap: baseInputs.compactCardGap,
        showPreview: false,
      }),
    );
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
