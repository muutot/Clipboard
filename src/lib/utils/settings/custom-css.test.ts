import { describe, expect, it } from "vitest";
import {
  compileCustomCss,
  isCustomCssRecoveryShortcut,
  normalizeCustomCss,
  MAX_CUSTOM_CSS_LENGTH,
} from "./custom-css";

describe("custom CSS", () => {
  it("preserves literal values and promotes declarations inside conditional rules", () => {
    const css = compileCustomCss(
      '.clip-card { content: "!important; }"; color: red; } @media (min-width: 1px) { :root { --accent: #123456; } }',
    );
    expect(css).toContain('content: "!important; }" !important');
    expect(css).toContain("color: red !important");
    expect(css).toContain("--accent: #123456 !important");
    expect(document.querySelector("style[media='not all']")).toBeNull();
  });
  it("promotes nested declarations and preserves their order around conditional rules", () => {
    const css = compileCustomCss(
      ".probe { color: red; @media (min-width: 1px) { background: blue; } color: green; }",
    );
    expect(css).toContain("color: red !important");
    expect(css).toContain("background: blue !important");
    expect(css).toContain("color: green !important");
    expect(css.indexOf("color: green")).toBeGreaterThan(css.indexOf("@media"));
  });
  it("does not add importance to keyframes or font descriptors", () => {
    const css = compileCustomCss(
      '@keyframes fade { from { opacity: 0; } to { opacity: 1; } } @font-face { font-family: custom; src: local("Arial"); }',
    );
    expect(css).toContain("@keyframes fade");
    expect(css).not.toContain("!important");
  });
  it("bounds input without trimming valid code or truncating it into a different rule", () => {
    const source = "/* 中文 */\n:root { --accent: #123456; }\n";
    expect(normalizeCustomCss(source)).toBe(source);
    expect(normalizeCustomCss("x".repeat(MAX_CUSTOM_CSS_LENGTH + 1))).toBe("");
    expect(normalizeCustomCss(null)).toBe("");
    expect(() => compileCustomCss("x".repeat(MAX_CUSTOM_CSS_LENGTH + 1))).toThrow("65,536");
  });
  it("reserves only the documented recovery chord", () => {
    expect(
      isCustomCssRecoveryShortcut(
        new KeyboardEvent("keydown", { code: "F12", ctrlKey: true, shiftKey: true }),
      ),
    ).toBe(true);
    expect(
      isCustomCssRecoveryShortcut(
        new KeyboardEvent("keydown", { code: "F12", metaKey: true, shiftKey: true }),
      ),
    ).toBe(true);
    expect(
      isCustomCssRecoveryShortcut(new KeyboardEvent("keydown", { code: "F12", shiftKey: true })),
    ).toBe(false);
    expect(
      isCustomCssRecoveryShortcut(
        new KeyboardEvent("keydown", { code: "F12", ctrlKey: true, shiftKey: true, altKey: true }),
      ),
    ).toBe(false);
  });
});
