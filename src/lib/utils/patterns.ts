import type { QuickAction } from "$lib/services/clipboard";

export const EMAIL_RE = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g;
export const URL_RE = /https?:\/\/[^\s)]+/g;
export const PHONE_RE = /(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g;
export const COLOR_RE = /#(?:[0-9a-fA-F]{3}){1,2}\b/g;

export function extractEmails(text: string): string[] {
  return [...new Set(text.match(EMAIL_RE) ?? [])];
}

export function extractUrls(text: string): string[] {
  return [...new Set(text.match(URL_RE) ?? [])];
}

export function extractPhones(text: string): string[] {
  return [...new Set(text.match(PHONE_RE) ?? [])];
}

export function extractColors(text: string): string[] {
  return [...new Set(text.match(COLOR_RE) ?? [])];
}

export function parseIsoDate(value: string): Date | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  if (year < 1 || month < 1 || month > 12 || day < 1) return null;
  const date = new Date(0);
  date.setUTCHours(12, 0, 0, 0);
  date.setUTCFullYear(year, month - 1, day);
  if (
    date.getUTCFullYear() !== year ||
    date.getUTCMonth() !== month - 1 ||
    date.getUTCDate() !== day
  ) {
    return null;
  }
  return date;
}

function normalizeInlineDate(year: number, month: number, day: number): string | null {
  const isoDate = `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
  return parseIsoDate(isoDate) ? isoDate : null;
}

export function extractDates(text: string): string[] {
  const values: string[] = [];
  const add = (value: string | null) => {
    if (value && !values.includes(value)) values.push(value);
  };

  for (const match of text.matchAll(/\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b/g)) {
    add(normalizeInlineDate(Number(match[1]), Number(match[2]), Number(match[3])));
  }
  for (const match of text.matchAll(/\b(\d{4})年(\d{1,2})月(\d{1,2})日/g)) {
    add(normalizeInlineDate(Number(match[1]), Number(match[2]), Number(match[3])));
  }
  for (const match of text.matchAll(/\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b/g)) {
    const first = Number(match[1]);
    const second = Number(match[2]);
    const year = Number(match[3]);
    const dayFirst = normalizeInlineDate(year, second, first);
    const monthFirst = normalizeInlineDate(year, first, second);
    if (dayFirst && monthFirst && dayFirst !== monthFirst) continue;
    add(dayFirst ?? monthFirst);
  }

  return values;
}

function findFirstDate(text: string): string | undefined {
  const m1 = text.match(/\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b/);
  if (m1) {
    const d = normalizeInlineDate(Number(m1[1]), Number(m1[2]), Number(m1[3]));
    if (d) return d;
  }
  const m2 = text.match(/\b(\d{4})年(\d{1,2})月(\d{1,2})日/);
  if (m2) {
    const d = normalizeInlineDate(Number(m2[1]), Number(m2[2]), Number(m2[3]));
    if (d) return d;
  }
  const re = /\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b/g;
  re.lastIndex = 0;
  for (const m of text.matchAll(re)) {
    const first = Number(m[1]);
    const second = Number(m[2]);
    const year = Number(m[3]);
    const dayFirst = normalizeInlineDate(year, second, first);
    const monthFirst = normalizeInlineDate(year, first, second);
    if (dayFirst && monthFirst && dayFirst !== monthFirst) continue;
    return dayFirst ?? monthFirst ?? undefined;
  }
}

export function detectQuickActions(text: string, firstOnly?: boolean): QuickAction[] {
  if (firstOnly) {
    const actions: QuickAction[] = [];

    const email = text.match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/);
    if (email)
      actions.push({
        label: `Send email to ${email[0]}`,
        actionType: "open",
        payload: `mailto:${email[0]}`,
        kind: "email",
      });

    const phone = text.match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/);
    if (phone)
      actions.push({
        label: `Call ${phone[0]}`,
        actionType: "open",
        payload: `tel:${phone[0].replace(/[^+\d]/g, "")}`,
        kind: "phone",
      });

    const url = text.match(/https?:\/\/[^\s)]+/);
    if (url)
      actions.push({ label: `Open ${url[0]}`, actionType: "open", payload: url[0], kind: "url" });

    const color = text.match(/#(?:[0-9a-fA-F]{3}){1,2}\b/);
    if (color)
      actions.push({
        label: `Copy color ${color[0]}`,
        actionType: "copy",
        payload: color[0],
        kind: "color",
      });

    const date = findFirstDate(text);
    if (date)
      actions.push({
        label: `View date ${date}`,
        actionType: "viewDate",
        payload: date,
        kind: "date",
      });

    return actions;
  }

  const dedupe = (items: QuickAction[]) => {
    const seen = new Set<string>();
    return items.filter((a) => {
      const key = `${a.actionType}:${a.payload}`;
      if (seen.has(key)) return false;
      seen.add(key);
      return true;
    });
  };

  return dedupe([
    ...extractEmails(text).map((v) => ({
      label: `Send email to ${v}`,
      actionType: "open" as const,
      payload: `mailto:${v}`,
      kind: "email" as const,
    })),
    ...extractPhones(text).map((v) => ({
      label: `Call ${v}`,
      actionType: "open" as const,
      payload: `tel:${v.replace(/[^+\d]/g, "")}`,
      kind: "phone" as const,
    })),
    ...extractUrls(text).map((v) => ({
      label: `Open ${v}`,
      actionType: "open" as const,
      payload: v,
      kind: "url" as const,
    })),
    ...extractDates(text).map((v) => ({
      label: `View date ${v}`,
      actionType: "viewDate" as const,
      payload: v,
      kind: "date" as const,
    })),
    ...extractColors(text).map((v) => ({
      label: `Copy color ${v}`,
      actionType: "copy" as const,
      payload: v,
      kind: "color" as const,
    })),
  ]);
}
