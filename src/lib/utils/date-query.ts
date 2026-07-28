export interface DateRange {
  from: number;
  to: number;
}

const dayMs = 24 * 60 * 60 * 1_000;

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
    resolver: (now) => ({ from: startOfDay(now - dayMs), to: endOfDay(now - dayMs) }),
  },
  {
    regex: /^(本周|this week|这周)$/i,
    resolver: (now) => ({ from: startOfWeek(now), to: endOfDay(now) }),
  },
  {
    regex: /^(上周|last week)$/i,
    resolver: (now) => {
      const lastWeekStart = startOfWeek(now - 7 * dayMs);
      return { from: lastWeekStart, to: endOfDay(lastWeekStart + 6 * dayMs) };
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
      d.setMonth(d.getMonth() - 1);
      d.setDate(1);
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
