import type { QuickAction } from "$lib/services/clipboard";

export const EMAIL_RE = /[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g;
export const URL_RE = /https?:\/\/[^\s)\u3000-\u303F\uFF00-\uFFEF]+/g;
export const PHONE_RE = /(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g;
/** HEX colors: 3/4-digit shorthand plus 6/8-digit #RRGGBB[AA] forms, matching
 * the quickActionKind fallback classifier in this file. Alternation is
 * longest-first so 8-digit values are not truncated to 6 by the boundary. */
export const COLOR_RE = /#(?:[0-9a-fA-F]{8}|[0-9a-fA-F]{6}|[0-9a-fA-F]{4}|[0-9a-fA-F]{3})\b/g;

/** ASCII sentence punctuation commonly glued to a copied URL; it ends the
 * sentence, not the address. Full-width/CJK punctuation never appears bare
 * in a valid URL, so URL_RE itself stops there. */
const URL_TRAILING_PUNCT = /[.,;:!?"'»…]+$/;

function normalizeUrl(raw: string): string {
  return raw.replace(URL_TRAILING_PUNCT, "");
}

export function extractEmails(text: string): string[] {
  return [...new Set(text.match(EMAIL_RE) ?? [])];
}

export function extractUrls(text: string): string[] {
  return [...new Set((text.match(URL_RE) ?? []).map(normalizeUrl))];
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
  // Every format scans all of its matches and skips candidates that fail
  // calendar validation: returning early on an invalid first candidate
  // (e.g. 9999-99-99) would miss a valid date later in the text.
  for (const m of text.matchAll(/\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b/g)) {
    const d = normalizeInlineDate(Number(m[1]), Number(m[2]), Number(m[3]));
    if (d) return d;
  }
  for (const m of text.matchAll(/\b(\d{4})年(\d{1,2})月(\d{1,2})日/g)) {
    const d = normalizeInlineDate(Number(m[1]), Number(m[2]), Number(m[3]));
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
    const hit = dayFirst ?? monthFirst;
    // An invalid candidate (neither reading parses) must be skipped, not
    // returned as `undefined`: early-returning here would abandon the
    // remaining matches even though a later one is a valid date.
    if (hit) return hit;
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

    const url = text.match(URL_RE)?.[0];
    if (url) {
      const normalized = normalizeUrl(url);
      actions.push({
        label: `Open ${normalized}`,
        actionType: "open",
        payload: normalized,
        kind: "url",
      });
    }

    const color = text.match(COLOR_RE)?.[0];
    if (color)
      actions.push({
        label: `Copy color ${color}`,
        actionType: "copy",
        payload: color,
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

/**
 * Resolves the icon/category bucket for a detected content action, preferring
 * the backend-provided `kind` and falling back to payload/action inspection.
 */
export function quickActionKind(
  action: QuickAction,
): "url" | "email" | "phone" | "date" | "color" | "copy" {
  if (action.kind) return action.kind;
  if (action.actionType === "viewDate") return "date";
  if (action.payload.startsWith("mailto:")) return "email";
  if (action.payload.startsWith("tel:")) return "phone";
  if (
    action.actionType === "open" &&
    (action.payload.startsWith("http://") || action.payload.startsWith("https://"))
  )
    return "url";
  if (
    action.actionType === "copy" &&
    /^(?:#[0-9a-fA-F]{3}|#[0-9a-fA-F]{4}|#[0-9a-fA-F]{6}|#[0-9a-fA-F]{8})$/.test(action.payload)
  )
    return "color";
  return "copy";
}
