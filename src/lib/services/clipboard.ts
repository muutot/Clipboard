import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";
import { getLocale, resolvePath } from "$lib/i18n";
import zhCN from "$lib/i18n/locales/zh-CN";
import en from "$lib/i18n/locales/en";

const locales = { "zh-CN": zhCN, en };

export async function loadClipboardHistory(
  limit = 100,
  offset = 0,
): Promise<ClipboardItem[] | null> {
  if (!isTauriRuntime()) return null;

  const records = await invoke<PersistedClipboardItem[]>("list_clipboard_items", {
    limit,
    offset,
  });

  return records.map(toClipboardItem);
}

export async function searchClipboardHistory(
  query: string,
  limit = 100,
): Promise<ClipboardItem[] | null> {
  if (!isTauriRuntime()) return null;

  const records = await invoke<PersistedClipboardItem[]>("search_clipboard_items", {
    query,
    limit,
  });

  return records.map(toClipboardItem);
}

export async function persistFavorite(id: string, isFavorite: boolean): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("set_clipboard_item_favorite", { id, isFavorite });
}

export async function persistDelete(id: string): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("delete_clipboard_item", { id });
}

export async function persistBatchFavorite(
  ids: string[],
  isFavorite: boolean,
): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("batch_set_favorite", { ids, isFavorite });
}

export async function persistBatchDelete(ids: string[]): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("batch_delete_clipboard_items", { ids });
}

export async function listSourceApplications(): Promise<string[] | null> {
  if (!isTauriRuntime()) return null;

  return invoke<string[]>("list_source_applications");
}

export interface ContentActions {
  hasEmail: boolean;
  hasUrl: boolean;
  hasPhone: boolean;
  hasColor: boolean;
  emails: string[];
  urls: string[];
  phones: string[];
  colors: string[];
}

export async function detectContentActions(
  contentId: string,
): Promise<ContentActions | null> {
  if (!isTauriRuntime()) return null;

  return invoke<ContentActions>("detect_content_actions", { contentId });
}

function toClipboardItem(record: PersistedClipboardItem): ClipboardItem {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const sourceApp = record.sourceApp?.trim()
    || resolvePath(messages, "app.name");

  const fileLabel = locale === "zh-CN" ? "个文件" : " file(s)";
  const imageLabel = locale === "zh-CN" ? "图片记录" : "Image record";

  return {
    id: record.id,
    kind: record.kind,
    title: record.title,
    preview: buildPreview(record, fileLabel, imageLabel),
    sourceApp,
    sourceTone: sourceTone(sourceApp, locale),
    sizeLabel: formatSizeSimple(record),
    createdAt: record.createdAtMs,
    favorite: record.isFavorite,
    fileName:
      record.kind === "file" ? fileNameFromPath(record.resourcePath) || record.title : undefined,
    previewPath: record.previewPath,
    resourcePath: record.resourcePath,
    textContent: record.textContent,
    iconPath: record.iconPath,
  };
}

function buildPreview(
  record: PersistedClipboardItem,
  fileLabel: string, imageLabel: string,
): string {
  if (record.textContent && record.textContent !== record.title) {
    return record.textContent;
  }

  if (record.kind === "file") return `1 ${fileLabel}`;
  if (record.kind === "image") return imageLabel;
  return "";
}

export function formatTextLength(length: number): string {
  const locale = getLocale();
  if (locale === "zh-CN") return `${length} 个字符`;
  return `${length} chars`;
}

function formatSizeSimple(record: PersistedClipboardItem): string {
  if (record.kind === "text" || record.kind === "link") {
    return formatTextLength((record.textContent || record.title).length);
  }

  const units = ["B", "KB", "MB", "GB"];
  let value = record.sizeBytes;
  let unitIndex = 0;

  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }

  const precision = unitIndex === 0 || value >= 100 ? 0 : 1;
  return `${value.toFixed(precision)} ${units[unitIndex]}`;
}

function fileNameFromPath(path: string | null): string | undefined {
  return path?.split(/[\\/]/).filter(Boolean).pop();
}

function sourceTone(sourceApp: string, locale: string): ClipboardItem["sourceTone"] {
  const normalized = sourceApp.toLocaleLowerCase();
  const unknownLabel = locale === "zh-CN" ? "未知来源" : "unknown";

  if (normalized === unknownLabel) return "neutral";
  if (normalized.includes("codex")) return "violet";
  if (normalized.includes("browser") || normalized.includes("chrome")) return "blue";
  return "red";
}
