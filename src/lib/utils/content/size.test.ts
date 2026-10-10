import { describe, expect, it } from "vitest";
import { SIZE_UNIT_MULTIPLIERS, SIZE_UNIT_OPTIONS, fromDisplaySize, toDisplaySize } from "./size";

describe("size", () => {
  it("uses binary multipliers", () => {
    expect(SIZE_UNIT_MULTIPLIERS.byte).toBe(1);
    expect(SIZE_UNIT_MULTIPLIERS.KB).toBe(1024);
    expect(SIZE_UNIT_MULTIPLIERS.MB).toBe(1024 ** 2);
    expect(SIZE_UNIT_MULTIPLIERS.GB).toBe(1024 ** 3);
  });

  it("exposes the display units used by size settings", () => {
    expect(SIZE_UNIT_OPTIONS.map((option) => option.value)).toEqual(["byte", "KB", "MB", "GB"]);
  });

  it("round-trips bytes through a display unit", () => {
    const bytes = 5 * 1024 * 1024 + 512;
    const display = toDisplaySize(bytes, "MB");
    expect(display).toBe(5);
    // The round trip is lossy by design: the display value is an integer.
    expect(fromDisplaySize(display, "MB")).toBe(5 * 1024 * 1024);
  });

  it("rounds half up in both directions", () => {
    expect(toDisplaySize(1536, "KB")).toBe(2); // 1.5 KB → 2
    expect(toDisplaySize(1023, "KB")).toBe(1);
    expect(fromDisplaySize(1.5 as number, "KB")).toBe(1536);
    expect(fromDisplaySize(0.4, "MB")).toBe(Math.round(0.4 * 1024 * 1024));
  });

  it("treats unknown units as plain bytes", () => {
    expect(toDisplaySize(999, "TiB")).toBe(999);
    expect(fromDisplaySize(7, "")).toBe(7);
  });

  it("handles byte units without scaling", () => {
    expect(toDisplaySize(123, "byte")).toBe(123);
    expect(fromDisplaySize(456, "byte")).toBe(456);
  });
});
