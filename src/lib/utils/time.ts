import { getLocale } from "$lib/i18n";

const units = [
  { unit: "year", milliseconds: 365 * 24 * 60 * 60 * 1_000 },
  { unit: "month", milliseconds: 30 * 24 * 60 * 60 * 1_000 },
  { unit: "week", milliseconds: 7 * 24 * 60 * 60 * 1_000 },
  { unit: "day", milliseconds: 24 * 60 * 60 * 1_000 },
  { unit: "hour", milliseconds: 60 * 60 * 1_000 },
  { unit: "minute", milliseconds: 60 * 1_000 },
] as const;

const justNowLabels: Record<string, string> = {
  "zh-CN": "刚刚",
  en: "just now",
};

export function formatRelativeTime(timestamp: number, currentTime = Date.now()): string {
  const locale = getLocale();
  const relativeTime = new Intl.RelativeTimeFormat(locale, { numeric: "auto" });
  const difference = timestamp - currentTime;

  if (Math.abs(difference) < 45_000) {
    return justNowLabels[locale] ?? justNowLabels.en;
  }

  for (const { unit, milliseconds } of units) {
    if (Math.abs(difference) >= milliseconds) {
      return relativeTime.format(
        Math.round(difference / milliseconds),
        unit as Intl.RelativeTimeFormatUnit,
      );
    }
  }

  return relativeTime.format(Math.round(difference / 1_000), "second");
}
