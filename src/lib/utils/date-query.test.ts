import { afterEach, describe, expect, it, vi } from "vitest";
import { endOfDay, parseDateQuery, startOfDay, startOfWeek } from "./date-query";

const DAY = 24 * 60 * 60 * 1000;

function ts(year: number, month: number, day: number, hours = 12): number {
  return new Date(year, month - 1, day, hours, 0, 0, 0).getTime();
}

afterEach(() => {
  vi.useRealTimers();
});

describe("day boundaries", () => {
  it("startOfDay and endOfDay bound the same local day", () => {
    const now = ts(2026, 8, 25, 15);
    const from = startOfDay(now);
    const to = endOfDay(now);
    expect(from).toBe(ts(2026, 8, 25, 0));
    expect(to).toBe(ts(2026, 8, 25, 23) + 59 * 60_000 + 59_999);
    expect(from <= now && now <= to).toBe(true);
  });
});

describe("startOfWeek", () => {
  it("returns Monday for a mid-week date", () => {
    // 2026-08-25 is a Tuesday.
    const monday = startOfWeek(ts(2026, 8, 25));
    expect(new Date(monday).getDay()).toBe(1);
    expect(monday).toBe(ts(2026, 8, 24, 0));
  });

  it("maps Sunday to the previous Monday", () => {
    // 2026-08-30 is a Sunday.
    const monday = startOfWeek(ts(2026, 8, 30));
    expect(new Date(monday).getDay()).toBe(1);
    expect(monday).toBe(ts(2026, 8, 24, 0));
  });

  it("already returns the same day for a Monday", () => {
    expect(startOfWeek(ts(2026, 8, 24))).toBe(ts(2026, 8, 24, 0));
  });
});

describe("parseDateQuery", () => {
  it("returns null for empty or unrecognized queries", () => {
    expect(parseDateQuery("")).toBeNull();
    expect(parseDateQuery("   ")).toBeNull();
    expect(parseDateQuery("hello world")).toBeNull();
  });

  it("parses today in English and Chinese case-insensitively", () => {
    vi.setSystemTime(ts(2026, 8, 25, 15));
    const expected = { from: startOfDay(Date.now()), to: endOfDay(Date.now()) };
    expect(parseDateQuery("today")).toEqual(expected);
    expect(parseDateQuery("TODAY")).toEqual(expected);
    expect(parseDateQuery("今天")).toEqual(expected);
    expect(parseDateQuery(" 今天 ")).toEqual(expected);
  });

  it("parses yesterday relative to now", () => {
    vi.setSystemTime(ts(2026, 8, 25, 15));
    const expected = { from: startOfDay(Date.now() - DAY), to: endOfDay(Date.now() - DAY) };
    expect(parseDateQuery("yesterday")).toEqual(expected);
    expect(parseDateQuery("昨天")).toEqual(expected);
  });

  it("parses this week starting Monday through end of today", () => {
    vi.setSystemTime(ts(2026, 8, 25, 15)); // Tuesday
    const range = parseDateQuery("this week")!;
    expect(range.from).toBe(ts(2026, 8, 24, 0));
    expect(range.to).toBe(endOfDay(Date.now()));
    expect(parseDateQuery("本周")).toEqual(range);
  });

  it("parses last week as the full previous Monday..Sunday window", () => {
    vi.setSystemTime(ts(2026, 8, 25, 15)); // Tuesday
    const range = parseDateQuery("last week")!;
    expect(range.from).toBe(ts(2026, 8, 17, 0));
    expect(range.to).toBe(endOfDay(ts(2026, 8, 23, 12)));
    expect(parseDateQuery("上周")).toEqual(range);
  });

  it("parses this month from the first of the month", () => {
    vi.setSystemTime(ts(2026, 8, 25, 15));
    const range = parseDateQuery("this month")!;
    expect(range.from).toBe(ts(2026, 8, 1, 0));
    expect(range.to).toBe(endOfDay(Date.now()));
    expect(parseDateQuery("本月")).toEqual(range);
  });

  it("parses last month across its exact boundaries including shorter months", () => {
    vi.setSystemTime(ts(2026, 3, 15, 9)); // March: previous month is February
    const range = parseDateQuery("last month")!;
    expect(range.from).toBe(ts(2026, 2, 1, 0));
    expect(range.to).toBe(endOfDay(ts(2026, 2, 28, 12)));
    expect(parseDateQuery("上个月")).toEqual(range);

    // A month after a 31-day month resolves the boundary via Date rollover.
    vi.setSystemTime(ts(2026, 4, 2, 9));
    const marchRange = parseDateQuery("上月")!;
    expect(marchRange.from).toBe(ts(2026, 3, 1, 0));
    expect(marchRange.to).toBe(endOfDay(ts(2026, 3, 31, 12)));
  });
});
