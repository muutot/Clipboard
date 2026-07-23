const relativeTime = new Intl.RelativeTimeFormat("zh-CN", { numeric: "auto" });

const units = [
  { unit: "year", milliseconds: 365 * 24 * 60 * 60 * 1_000 },
  { unit: "month", milliseconds: 30 * 24 * 60 * 60 * 1_000 },
  { unit: "week", milliseconds: 7 * 24 * 60 * 60 * 1_000 },
  { unit: "day", milliseconds: 24 * 60 * 60 * 1_000 },
  { unit: "hour", milliseconds: 60 * 60 * 1_000 },
  { unit: "minute", milliseconds: 60 * 1_000 },
] as const;

export function formatRelativeTime(timestamp: number, currentTime = Date.now()): string {
  const difference = timestamp - currentTime;

  if (Math.abs(difference) < 45_000) {
    return "刚刚";
  }

  for (const { unit, milliseconds } of units) {
    if (Math.abs(difference) >= milliseconds) {
      return relativeTime.format(Math.round(difference / milliseconds), unit);
    }
  }

  return relativeTime.format(Math.round(difference / 1_000), "second");
}
