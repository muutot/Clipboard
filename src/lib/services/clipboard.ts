import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";
import type { ClipboardItem, PersistedClipboardItem } from "$lib/types/clipboard";
import { getLocale, resolvePath } from "$lib/i18n";
import zhCN from "$lib/i18n/locales/zh-CN";
import en from "$lib/i18n/locales/en";

const locales = { "zh-CN": zhCN, en };

export async function writeClipboardText(text: string): Promise<void> {
  if (isTauriRuntime()) {
    await invoke("mark_self_triggered", { text });
  }
  await navigator.clipboard.writeText(text);
}

export async function writeClipboardImage(
  blob: Blob,
  resourcePath?: string | null,
  contentHash?: string,
): Promise<void> {
  if (isTauriRuntime() && (resourcePath || contentHash)) {
    try {
      await invoke("mark_self_triggered_image", {
        resourcePath: resourcePath ?? null,
        contentHash: contentHash ?? null,
      });
    } catch (error) {
      console.warn("Unable to register image self-trigger", error);
    }
  }

  await navigator.clipboard.write([new ClipboardItem({ [blob.type || "image/png"]: blob })]);
}

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

/** Loads the persisted recycle-bin page. Deleted records are marked locally
 * because the regular ClipboardItem payload intentionally represents active
 * history only. */
export async function loadDeletedClipboardHistory(
  limit = 100,
  offset = 0,
): Promise<ClipboardItem[] | null> {
  if (!isTauriRuntime()) return null;

  const records = await invoke<PersistedClipboardItem[]>("list_deleted_clipboard_items", {
    limit,
    offset,
  });

  return records.map((record) => ({ ...toClipboardItem(record), deleted: true }));
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

  return invoke<boolean>("soft_delete_clipboard_item", { id });
}

export async function persistHardDelete(id: string): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("delete_clipboard_item", { id });
}

export async function persistRestore(id: string): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("restore_clipboard_item", { id });
}

export async function persistBatchRestore(ids: string[]): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("batch_restore_clipboard_items", { ids });
}

export async function persistPermanentDelete(id: string): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("permanently_delete_clipboard_item", { id });
}

export async function persistBatchPermanentDelete(ids: string[]): Promise<boolean | null> {
  if (!isTauriRuntime()) return null;

  return invoke<boolean>("batch_permanently_delete_clipboard_items", { ids });
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

export interface QuickAction {
  label: string;
  actionType: "open" | "copy";
  payload: string;
}

export async function detectContentActions(text: string): Promise<QuickAction[] | null> {
  if (!isTauriRuntime()) return null;

  return invoke<QuickAction[]>("detect_content_actions", { text });
}

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

function toClipboardItem(record: PersistedClipboardItem): ClipboardItem {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const sourceApp = record.sourceApp?.trim() || resolvePath(messages, "app.name");

  const fileLabel = locale === "zh-CN" ? "个文件" : " file(s)";
  const imageLabel = locale === "zh-CN" ? "图片记录" : "Image record";

  let imageMeta: { width: number; height: number } | undefined;
  let fileMeta: { name: string; size: number }[] | undefined;
  if (record.metadataJson) {
    try {
      const meta = JSON.parse(record.metadataJson);
      if (
        record.kind === "image" &&
        typeof meta.width === "number" &&
        typeof meta.height === "number"
      ) {
        imageMeta = { width: meta.width, height: meta.height };
      }
      if (record.kind === "file" && Array.isArray(meta.files)) {
        fileMeta = meta.files;
      }
    } catch {
      /* ignore */
    }
  }

  return {
    id: record.id,
    kind: record.kind,
    title: record.title,
    preview: buildPreview(record, fileLabel, imageLabel),
    sourceApp,
    sourceTone: sourceTone(sourceApp, locale),
    sizeLabel: formatSizeSimple(record),
    sizeBytes: record.sizeBytes,
    createdAt: record.createdAtMs,
    favorite: record.isFavorite,
    customTitle: isCustomClipboardTitle(record),
    fileName:
      record.kind === "file"
        ? fileMeta?.[0]?.name || fileNameFromPath(record.resourcePath) || record.title
        : undefined,
    imageMeta,
    fileMeta,
    previewPath: record.previewPath,
    resourcePath: record.resourcePath,
    contentHash: record.contentHash,
    textContent: record.textContent,
    iconPath: record.iconPath,
    metadataJson: record.metadataJson,
  };
}

function buildPreview(
  record: PersistedClipboardItem,
  fileLabel: string,
  imageLabel: string,
): string {
  if (record.textContent && record.textContent !== record.title) {
    const lines = record.textContent.split("\n");
    return lines.length > 1 ? lines[1] : "";
  }

  if (record.kind === "file") {
    if (record.textContent && record.textContent.startsWith("[")) {
      try {
        const paths = JSON.parse(record.textContent) as string[];
        if (paths.length > 1) return `${paths.length} ${fileLabel}`;
      } catch {
        /* ignore */
      }
    }
    return `1 ${fileLabel}`;
  }
  if (record.kind === "image") return imageLabel;
  return "";
}

export function formatTextLength(length: number): string {
  const locale = getLocale();
  if (locale === "zh-CN") return `${length} 个字符`;
  return `${length} chars`;
}

export function formatSizeSimple(record: PersistedClipboardItem): string {
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
