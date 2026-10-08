import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import MarkdownPreview from "./MarkdownPreview.svelte";

const cleanup: Array<() => void> = [];
afterEach(() => {
  while (cleanup.length) cleanup.pop()?.();
});

function render(content: string) {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(MarkdownPreview, { target, props: { content } });
  cleanup.push(() => {
    unmount(component);
    target.remove();
  });
  flushSync();
  return target;
}

describe("MarkdownPreview", () => {
  it("renders consecutive quoted lines as a blockquote", () => {
    const target = render("> first\n> **second**");
    expect(target.querySelectorAll("blockquote")).toHaveLength(1);
    expect(target.querySelector("blockquote strong")?.textContent).toBe("second");
  });

  it("preserves fenced code text, indentation and blank lines without parsing markup", () => {
    const code = "  **bold**\n\n[x](https://example.com)\n> quote\n- list\n<tag>\n";
    const target = render("```text\n" + code + "```\n\nafter");
    expect(target.querySelector("pre code")?.textContent).toBe(code);
    expect(target.querySelector("pre code")?.children).toHaveLength(0);
    expect(target.querySelector("pre p, pre a, pre strong, pre ul")).toBeNull();
    expect(target.querySelector(".md-p")?.textContent).toBe("after");
  });

  it("keeps inline code literal", () => {
    const target = render("before `**bold** [x](https://example.com)` after");
    expect(target.querySelector("code")?.textContent).toBe("**bold** [x](https://example.com)");
    expect(target.querySelector("code")?.children).toHaveLength(0);
  });

  it("wraps lists even when items contain inline formatting", () => {
    const target = render("- **first**\n- [second](https://example.com)\n\n1. `third`");
    expect(target.querySelectorAll("ul > li")).toHaveLength(2);
    expect(target.querySelectorAll("ol > li")).toHaveLength(1);
    expect(target.querySelector("ul strong")?.textContent).toBe("first");
  });

  it("escapes raw HTML and image attributes while allowing safe URLs", () => {
    const target = render(
      '<script>alert(1)</script>\n![`alt` "quoted"](https://example.com/a.png)',
    );
    expect(target.querySelector("script")).toBeNull();
    expect(target.querySelector("img")?.alt).toBe('`alt` "quoted"');
    expect(target.querySelector("img")?.getAttribute("src")).toBe("https://example.com/a.png");
    expect(target.querySelector("img")?.attributes.length).toBe(3);
  });

  it("rejects active URL schemes and preserves their labels", () => {
    const target = render(
      "[bad](javascript:alert) ![bad image](data:image/png;base64,AAAA) [good](https://example.com?a=1&b=2)",
    );
    expect(target.querySelectorAll("a")).toHaveLength(1);
    expect(target.querySelector("img")).toBeNull();
    expect(target.querySelector("a")?.getAttribute("href")).toBe("https://example.com/?a=1&b=2");
    expect(target.textContent).toContain("bad image");
  });

  it("preserves formatting inside links and uses code literally inside emphasis", () => {
    const target = render("[**label**](https://example.com) **`literal`**");
    expect(target.querySelector("a strong")?.textContent).toBe("label");
    expect(target.querySelector("strong code")?.textContent).toBe("literal");
  });
});
