import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";

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

function toClipboardItem(record: PersistedClipboardItem): ClipboardItem {
  const sourceApp = record.sourceApp?.trim() || "未知来源";

  return {
    id: record.id,
    kind: record.kind,
    title: record.title,
    preview: buildPreview(record),
    sourceApp,
    sourceTone: sourceTone(sourceApp),
    sizeLabel: formatSize(record),
    createdAt: record.createdAtMs,
    favorite: record.isFavorite,
    fileName:
      record.kind === "file" ? fileNameFromPath(record.resourcePath) || record.title : undefined,
  };
}

function buildPreview(record: PersistedClipboardItem): string {
  if (record.textContent && record.textContent !== record.title) {
    return record.textContent;
  }

  if (record.kind === "file") return "1 个文件";
  if (record.kind === "image") return "图片记录";
  return "";
}

function formatSize(record: PersistedClipboardItem): string {
  if (record.kind === "text" || record.kind === "link") {
    return `${[...(record.textContent || record.title)].length} 个字符`;
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

function sourceTone(sourceApp: string): ClipboardItem["sourceTone"] {
  const normalized = sourceApp.toLocaleLowerCase();

  if (normalized === "未知来源") return "neutral";
  if (normalized.includes("codex")) return "violet";
  if (normalized.includes("browser") || normalized.includes("chrome")) return "blue";
  return "red";
}
