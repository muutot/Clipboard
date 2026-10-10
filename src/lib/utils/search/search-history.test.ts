import { describe, it, expect } from "vitest";
import {
  SEARCH_HISTORY_LIMIT,
  SEARCH_TERM_MAX_LENGTH,
  nextSearchHistory,
  normalizeSearchTerm,
  suggestionCandidate,
  loadSearchHistory,
  persistSearchHistory,
} from "./search-history";

class MemoryStorage implements Storage {
  private store = new Map<string, string>();
  length = 0;
  clear(): void {
    this.store.clear();
  }
  getItem(key: string): string | null {
    return this.store.get(key) ?? null;
  }
  key(index: number): string | null {
    return Array.from(this.store.keys())[index] ?? null;
  }
  removeItem(key: string): void {
    this.store.delete(key);
  }
  setItem(key: string, value: string): void {
    this.store.set(key, value);
  }
}

describe("search-history", () => {
  it("normalizes terms to trimmed and max length", () => {
    expect(normalizeSearchTerm("  hello  ")).toBe("hello");
    expect(normalizeSearchTerm("a".repeat(200)).length).toBe(SEARCH_TERM_MAX_LENGTH);
    expect(normalizeSearchTerm("   ")).toBe("");
  });

  it("loadSearchHistory parses, dedupes case-insensitively and limits", () => {
    const storage = new MemoryStorage();
    storage.setItem(
      "clipboard.search-history.v1",
      JSON.stringify([
        "Hello",
        "hello",
        " World ",
        "HELLO",
        123 as unknown as string,
        "",
        "extra",
        "a",
        "b",
        "c",
        "d",
        "e",
        "f",
        "g",
      ]),
    );
    const history = loadSearchHistory(storage);
    expect(history).toEqual(["Hello", "World", "extra", "a", "b", "c", "d", "e"]);
    expect(history.length).toBeLessThanOrEqual(SEARCH_HISTORY_LIMIT);
  });

  it("loadSearchHistory returns [] on invalid JSON or missing storage", () => {
    const storage = new MemoryStorage();
    storage.setItem("clipboard.search-history.v1", "not-json");
    expect(loadSearchHistory(storage)).toEqual([]);
    expect(loadSearchHistory(null)).toEqual([]);
  });

  it("persistSearchHistory slices to limit and stringifies", () => {
    const storage = new MemoryStorage();
    persistSearchHistory(storage, ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]);
    const raw = storage.getItem("clipboard.search-history.v1");
    expect(JSON.parse(raw!)).toEqual(["a", "b", "c", "d", "e", "f", "g", "h"]);
  });

  it("nextSearchHistory moves term to front and dedupes", () => {
    expect(nextSearchHistory(["a", "b", "c"], "b")).toEqual(["b", "a", "c"]);
    expect(nextSearchHistory(["a", "b"], "C")).toEqual(["C", "a", "b"]);
    expect(nextSearchHistory(["a", "b"], "a")).toEqual(["a", "b"]);
    expect(nextSearchHistory(["a"], "  ")).toEqual(["a"]);
    expect(nextSearchHistory([], "  hello  ")).toEqual(["hello"]);
  });

  it("suggestionCandidate normalizes spaces, aligns to query and validates length", () => {
    expect(suggestionCandidate(null)).toBeNull();
    expect(suggestionCandidate("")).toBeNull();
    expect(suggestionCandidate("hi")).toBe("hi");
    expect(suggestionCandidate("a")).toBeNull();
    expect(suggestionCandidate("  multiple   spaces  ")).toBe("multiple spaces");
    expect(suggestionCandidate("hello world", "world")).toBe("hello world");
    expect(suggestionCandidate("hello world", "missing")).toBeNull();
    expect(suggestionCandidate("prefix hello world", "hello", true)).toBe("hello world");
    // Long candidate forces align-to-query slicing even without the flag
    const long = "prefix ".repeat(30) + "target";
    expect(suggestionCandidate(long, "target")?.startsWith("target")).toBe(true);
    expect(suggestionCandidate(long)?.length).toBe(SEARCH_TERM_MAX_LENGTH);
  });
});
