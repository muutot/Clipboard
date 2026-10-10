// Pure display-text helpers shared by the card, the detail panel, the float
// panel and the clipboard service. They live outside `$lib/services/clipboard`
// so layout and content utils can use them without depending on the service
// layer (and the i18n/Tauri imports that come with it).

import type { PersistedClipboardItem } from "$lib/types/clipboard";

/** True when the stored title was set by the user instead of generated from the
 * captured text: the metadata flag wins, then the legacy title comparison. */
export function isCustomClipboardTitle(
  record: Pick<PersistedClipboardItem, "title" | "textContent" | "metadataJson">,
): boolean {
  if (record.metadataJson) {
    try {
      const customTitle = JSON.parse(record.metadataJson)?.customTitle;
      if (typeof customTitle === "boolean") return customTitle;
    } catch {
      /* fall back to the legacy title rule */
    }
  }

  if (!record.textContent) return false;
  return record.title !== generatedClipboardTitle(record.textContent);
}

export function generatedClipboardTitle(text: string): string {
  return Array.from(text).slice(0, 200).join("");
}

/** First non-blank line of a text preview; empty when the text has none. */
export function getDisplayTitle(text: string): string {
  const match = /[^\r\n]+/.exec(text);
  return match ? match[0].trim() : "";
}

/** Everything after the first non-blank line, shown as the card's second line. */
export function getDisplayRemainingLines(text: string): string {
  const match = /[^\r\n]+/.exec(text);
  if (!match) return "";

  // 切出第一行后面的部分，直接调用原生 trimStart() 剥离开头所有不可见字符
  return text.slice(match.index + match[0].length).trimStart();
}
