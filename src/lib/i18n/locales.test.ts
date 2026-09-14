import { describe, expect, it } from "vitest";

import en from "./locales/en";
import zhCN from "./locales/zh-CN";

function leafKeys(value: unknown, prefix = ""): string[] {
  if (value === null || typeof value !== "object") return [prefix];
  return Object.entries(value as Record<string, unknown>).flatMap(([key, child]) =>
    leafKeys(child, prefix ? `${prefix}.${key}` : key),
  );
}

describe("i18n locales", () => {
  it("keeps en and zh-CN key sets in sync", () => {
    expect(leafKeys(en).sort()).toEqual(leafKeys(zhCN).sort());
  });

  it("resolves the labels that previously rendered as raw key paths", () => {
    expect(en.actions.save.length).toBeGreaterThan(0);
    expect(zhCN.actions.save.length).toBeGreaterThan(0);
    expect(en.detail.file.length).toBeGreaterThan(0);
    expect(zhCN.detail.file.length).toBeGreaterThan(0);
  });
});
