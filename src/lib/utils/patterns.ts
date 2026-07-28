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
