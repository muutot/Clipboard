import { beforeEach, describe, expect, it, vi } from "vitest";
import { refreshSourceMappingRules, resolveWebSourceName } from "./source-mapping.svelte";
import type { WebSourceMetadata } from "$lib/types/clipboard";

vi.mock("$lib/services/clipboard", () => ({
  getSourceMappingRules: vi.fn().mockResolvedValue([]),
}));

const item = (webSource?: WebSourceMetadata, sourceApp = "Google Chrome") => ({
  sourceApp,
  webSource,
});

beforeEach(() => {
  refreshSourceMappingRules([]);
});

describe("resolveWebSourceName", () => {
  it("prefers the first matching user rule over built-ins", () => {
    refreshSourceMappingRules([
      { domain: "", titleKeyword: "Rust", name: "Rust pages" },
      { domain: "github.com", titleKeyword: "", name: "GH" },
    ]);
    expect(resolveWebSourceName(item({ title: "Rust repo", domain: "github.com" }))).toBe(
      "Rust pages",
    );
    expect(resolveWebSourceName(item({ title: "Issue", domain: "github.com" }))).toBe("GH");
  });

  it("matches rule domains on the stored domain or its subdomains", () => {
    refreshSourceMappingRules([{ domain: "example.com", titleKeyword: "", name: "Example" }]);
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
