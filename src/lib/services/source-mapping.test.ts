import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getSourceMappingRules } from "$lib/services/clipboard";
import { createSourceMappingStore, type SourceMappingStore } from "./source-mapping.svelte";
import type { WebSourceMetadata } from "$lib/types/clipboard";

vi.mock("$lib/services/clipboard", () => ({
  getSourceMappingRules: vi.fn().mockResolvedValue([]),
}));

const item = (webSource?: WebSourceMetadata, sourceApp = "Google Chrome") => ({
  sourceApp,
  webSource,
});

let store: SourceMappingStore;
const resolveWebSourceName = (capture: Parameters<SourceMappingStore["resolveWebSourceName"]>[0]) =>
  store.resolveWebSourceName(capture);
beforeEach(() => {
  store = createSourceMappingStore();
  vi.mocked(getSourceMappingRules).mockResolvedValue([]);
});
afterEach(() => store.destroy());

describe("resolveWebSourceName", () => {
  it.each([
    ["github.com", "GitHub"],
    ["gitlab.com", "GitLab"],
    ["stackoverflow.com", "Stack Overflow"],
    ["mozilla.org", "MDN"],
    ["microsoft.com", "Microsoft"],
    ["wikipedia.org", "Wikipedia"],
    ["youtube.com", "YouTube"],
    ["bilibili.com", "Bilibili"],
    ["zhihu.com", "知乎"],
    ["juejin.cn", "掘金"],
    ["csdn.net", "CSDN"],
    ["v2ex.com", "V2EX"],
    ["reddit.com", "Reddit"],
    ["x.com", "X"],
    ["npmjs.com", "npm"],
    ["medium.com", "Medium"],
    ["notion.so", "Notion"],
  ])("keeps the complete built-in name for %s and its subdomains", (domain, name) => {
    expect(resolveWebSourceName(item({ title: "Page title", domain }))).toBe(name);
    expect(resolveWebSourceName(item({ title: "Page title", domain: `docs.${domain}` }))).toBe(
      name,
    );
  });

  it("lets user rules override a multi-word built-in name", async () => {
    vi.mocked(getSourceMappingRules).mockResolvedValue([
      { domain: "stackoverflow.com", titleKeyword: "", name: "SO answers" },
    ]);
    await store.refresh();
    expect(resolveWebSourceName(item({ title: "Question", domain: "stackoverflow.com" }))).toBe(
      "SO answers",
    );
    expect(
      resolveWebSourceName(item({ title: "Question", domain: "meta.stackoverflow.com" })),
    ).toBe("SO answers");
  });

  it("prefers the first matching user rule over built-ins", async () => {
    vi.mocked(getSourceMappingRules).mockResolvedValue([
      { domain: "", titleKeyword: "Rust", name: "Rust pages" },
      { domain: "github.com", titleKeyword: "", name: "GH" },
    ]);
    await store.refresh();
    expect(resolveWebSourceName(item({ title: "Rust repo", domain: "github.com" }))).toBe(
      "Rust pages",
    );
    expect(resolveWebSourceName(item({ title: "Issue", domain: "github.com" }))).toBe("GH");
  });

  it("matches rule domains on the stored domain or its subdomains", async () => {
    vi.mocked(getSourceMappingRules).mockResolvedValue([
      { domain: "example.com", titleKeyword: "", name: "Example" },
    ]);
    await store.refresh();
    expect(resolveWebSourceName(item({ title: "T", domain: "example.com" }))).toBe("Example");
    expect(resolveWebSourceName(item({ title: "T", domain: "docs.example.com" }))).toBe("Example");
    expect(resolveWebSourceName(item({ title: "T", domain: "notexample.com" }))).not.toBe(
      "Example",
    );
  });

  it("falls back to built-in domain names, then the truncated title", () => {
    expect(
      resolveWebSourceName(item({ title: "Very long page title", domain: "github.com" })),
    ).toBe("GitHub");
    const longTitle = "A".repeat(40);
    expect(resolveWebSourceName(item({ title: longTitle, domain: "example.org" }))).toBe(
      `${"A".repeat(24)}…`,
    );
    expect(resolveWebSourceName(item({ title: "Short", domain: "example.org" }))).toBe("Short");
  });

  it("returns the source app without web metadata", () => {
    expect(resolveWebSourceName(item(undefined, "Notepad"))).toBe("Notepad");
    expect(resolveWebSourceName(item({ title: "", domain: undefined }, "Notepad"))).toBe("Notepad");
  });
});
