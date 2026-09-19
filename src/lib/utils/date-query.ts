export interface DateRange {
  from: number;
  to: number;
}

/**
 * Shifts a timestamp by whole calendar days. Calendar arithmetic (unlike
 * subtracting `24h` epochs) is DST-safe: on the day after a spring-forward
 * transition the local day is only 23 hours, so `now - 24h` lands on the
 * wrong calendar day while this helper moves exactly one wall-clock day.
 */
export function shiftCalendarDays(ts: number, days: number): number {
  const d = new Date(ts);
  d.setDate(d.getDate() + days);
  return d.getTime();
}

export function startOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

export function endOfDay(ts: number): number {
  const d = new Date(ts);
  d.setHours(23, 59, 59, 999);
  return d.getTime();
}

export function startOfWeek(ts: number): number {
  const d = new Date(ts);
  const day = d.getDay();
  const diff = d.getDate() - day + (day === 0 ? -6 : 1);
  d.setDate(diff);
  d.setHours(0, 0, 0, 0);
  return d.getTime();
}

const patterns: Array<{ regex: RegExp; resolver: (now: number) => DateRange }> = [
  {
    regex: /^(今天|today)$/i,
    resolver: (now) => ({ from: startOfDay(now), to: endOfDay(now) }),
  },
  {
    regex: /^(昨天|yesterday)$/i,
    resolver: (now) => {
      const yesterday = shiftCalendarDays(now, -1);
      return { from: startOfDay(yesterday), to: endOfDay(yesterday) };
    },
  },
  {
    regex: /^(本周|this week|这周)$/i,
    resolver: (now) => ({ from: startOfWeek(now), to: endOfDay(now) }),
  },
  {
    regex: /^(上周|last week)$/i,
    resolver: (now) => {
      const lastWeekStart = startOfWeek(shiftCalendarDays(now, -7));
      return { from: lastWeekStart, to: endOfDay(shiftCalendarDays(lastWeekStart, 6)) };
    },
  },
  {
    regex: /^(本月|this month|这个月)$/i,
    resolver: (now) => {
      const d = new Date(now);
      d.setDate(1);
      d.setHours(0, 0, 0, 0);
      return { from: d.getTime(), to: endOfDay(now) };
    },
  },
  {
    regex: /^(上月|last month|上个月)$/i,
    resolver: (now) => {
      const d = new Date(now);
      // Normalize the day first so short months never roll the decremented
      // month back over (e.g. March 31 minus one month must be February 1,
      // not the rollover result March 2).
      d.setDate(1);
      d.setMonth(d.getMonth() - 1);
      const from = new Date(d.getFullYear(), d.getMonth(), 1).getTime();
      const to = endOfDay(new Date(d.getFullYear(), d.getMonth() + 1, 0).getTime());
      return { from, to };
    },
  },
];

export function parseDateQuery(query: string): DateRange | null {
  const trimmed = query.trim();
  if (!trimmed) return null;

  const now = Date.now();
  for (const { regex, resolver } of patterns) {
    if (regex.test(trimmed)) {
      return resolver(now);
    }
  }

  return null;
}
