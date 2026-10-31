import { describe, expect, it } from "vitest";
import { tokenizeCode } from "./code-tokenize";

const lineText = (line: { text: string }[]) => line.map((token) => token.text).join("");

describe("tokenizeCode line splitting", () => {
  it("renders a single-line snippet that never sees a newline boundary", () => {
    // Regression: the last (and here, only) line was never flushed into the
    // result, so the preview rendered the language chip but zero lines.
    const lines = tokenizeCode("32", "SQL");
    expect(lines).toHaveLength(1);
    expect(lineText(lines[0]!)).toBe("32");
  });

  it("keeps the last line when the content has no trailing newline", () => {
    const lines = tokenizeCode("const a = 1;\nconst b = 2;", "JavaScript");
    expect(lines).toHaveLength(2);
    expect(lineText(lines[1]!)).toBe("const b = 2;");
  });

  it("adds no phantom line for a trailing newline", () => {
    const lines = tokenizeCode("const a = 1;\n", "JavaScript");
    expect(lines).toHaveLength(1);
  });

  it("yields empty token rows for interior blank lines", () => {
    const lines = tokenizeCode("a\n\nb", "text");
    expect(lines).toHaveLength(3);
    expect(lines[1]).toEqual([]);
  });

  it("carries a cross-line span class onto the next line", () => {
    const lines = tokenizeCode("/* one\ntwo */", "JavaScript");
    expect(lines).toHaveLength(2);
    expect(lineText(lines[0]!)).toBe("/* one");
    expect(lineText(lines[1]!)).toBe("two */");
    expect(lines[1]![0]?.classes).toContain("hljs-comment");
  });

  it("falls back to plain lines for unknown languages", () => {
    const lines = tokenizeCode("just text", "NoSuchLanguage");
    expect(lines).toHaveLength(1);
    expect(lines[0]).toEqual([{ text: "just text", classes: "" }]);
  });
});
