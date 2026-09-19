import { describe, expect, it } from "vitest";

import { resolvePath } from "./index";
import en from "./locales/en";
import zhCN from "./locales/zh-CN";

function leafKeys(value: unknown, prefix = ""): string[] {
  if (value === null || typeof value !== "object") return [prefix];
  return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key),
  );
}

function resolves(source: unknown, path: string): boolean {
  let value: unknown = source;
  for (const key of path.split(".")) {
    if (value && typeof value === "object" && key in (value as Record<string, unknown>)) {
      value = (value as Record<string, unknown>)[key];
    } else {
      return false;
    }
  }
  return typeof value === "string";
}

/** Raw source files scanned for quoted dotted-path literals that must resolve. */
const SOURCE_FILES = import.meta.glob("/src/**/*.{svelte,ts,js}", {
  query: "?raw",
  import: "default",
  eager: true,
}) as Record<string, string>;

/** Collects dotted-path literals referenced from source (locales and tests excluded). */
function collectReferencedKeys(): Map<string, string[]> {
  // Quoted dotted paths ("section.key"). File extensions and $-prefixed
  // specifiers are not locale keys; style blocks never contain them.
  const LITERAL = /["'`]([a-zA-Z_][a-zA-Z0-9_]*(?:\.[a-zA-Z0-9_]+)+)["'`]/g;
  const NOT_A_KEY =
    /\.(png|jpe?g|gif|svg|webp|ico|json|ya?ml|toml|css|jsx?|tsx?|svelte|html?|md|txt|exe|dll|wasm|node|map)$/i;
  const referenced = new Map<string, string[]>();
  for (const [file, raw] of Object.entries(SOURCE_FILES)) {
    const base = file.split("/").pop() ?? file;
    if (file.includes("/i18n/locales/") || base.includes(".test.")) continue;
    const content = raw.replace(/<style[\s\S]*?<\/style>/g, "");
    for (const match of content.matchAll(LITERAL)) {
      const key = match[1];
      if (key.startsWith("$") || NOT_A_KEY.test(key)) continue;
      const list = referenced.get(key) ?? [];
      list.push(base);
      referenced.set(key, list);
    }
  }
  return referenced;
}

describe("i18n locales", () => {
  it("keeps en and zh-CN key sets in sync", () => {
    expect(leafKeys(en).sort()).toEqual(leafKeys(zhCN).sort());
  });

  it("replaces every placeholder and ignores $ patterns in values", () => {
    const source = { msg: "{title} x{title} y" };
    // `$&` in a user-supplied value must be inserted literally, not expand
    // to the matched placeholder, and every occurrence must be replaced.
    expect(resolvePath(source, "msg", { title: "a$&b" })).toBe("a$&b xa$&b y");
  });

  it("resolves the labels that previously rendered as raw key paths", () => {
    expect(en.actions.save.length).toBeGreaterThan(0);
    expect(zhCN.actions.save.length).toBeGreaterThan(0);
    expect(en.detail.file.length).toBeGreaterThan(0);
    expect(zhCN.detail.file.length).toBeGreaterThan(0);
  });

  it("resolves every dotted-path literal referenced from source", () => {
    // Dotted literals that are NOT locale keys: code property accesses, and
    // settings search entry IDs, which anchor panels via
    // data-settings-search-id/searchId instead of translating text.
    const NOT_LOCALE_KEYS = new Set([
      "Math.floor",
      "event.currentTarget",
      "event.key",
      "item.id",
      "window.localStorage",
      "about.info",
      "about.update",
      "font.base",
      "font.secondary",
      "font.tiny",
      "ocr.engine",
      "ocr.model",
      "recording.pause",
      "statistics.performance.uptime",
      "statistics.storage.database",
      "statistics.storage.files",
      "statistics.storage.images",
      "statistics.storage.links",
      "statistics.storage.text",
      "storage.transfer",
      "tags.autoTagRules",
      "tags.manage",
    ]);
    const missing: string[] = [];
    for (const [key, files] of collectReferencedKeys()) {
      if (NOT_LOCALE_KEYS.has(key)) continue;
      if (!resolves(en, key) || !resolves(zhCN, key)) {
        missing.push(`${key}  (${files.map((f) => f.split(/[\\/]/).pop()).join(", ")})`);
      }
    }
    expect(missing.sort()).toEqual([]);
  });
});
