import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";
import type {
  ClipboardItem,
  PersistedClipboardItem,
  ResourceFileMetadata,
  ResourceMetadata,
} from "$lib/types/clipboard";
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
  actionType: "open" | "copy" | "viewDate";
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

export function getDisplayTitle(title: string): string {
  const firstNonEmpty = title.split("\n").find((line) => line.trim() !== "");
  return firstNonEmpty ?? "";
}

export function getDisplayRemainingLines(text: string): string {
  const firstLine = getDisplayTitle(text);
  if (!firstLine) return "";
  const idx = text.indexOf(firstLine);
  if (idx === -1) return "";
  const after = text.slice(idx + firstLine.length);
  return after.startsWith("\n") ? after.slice(1) : after;
}

export function toClipboardItem(record: PersistedClipboardItem): ClipboardItem {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const sourceApp = record.sourceApp?.trim() || resolvePath(messages, "app.name");

  const fileLabel = locale === "zh-CN" ? "个文件" : " file(s)";
  const imageLabel = locale === "zh-CN" ? "图片记录" : "Image record";

  const resourceMetadata = parseResourceMetadata(record);
  const imageMeta =
    record.kind === "image" &&
    resourceMetadata?.width !== undefined &&
    resourceMetadata.height !== undefined
      ? { width: resourceMetadata.width, height: resourceMetadata.height }
      : undefined;
  const fileMeta = record.kind === "file" ? resourceMetadata?.files : undefined;
  const primaryFile = fileMeta?.[0];

  return {
    id: record.id,
    kind: record.kind,
    title: record.title,
    preview: buildPreview(record, fileLabel, imageLabel),
    sourceApp,
    sourceTone: sourceTone(sourceApp, locale),
    sizeLabel: formatSizeSimple(record),
    sizeBytes: record.sizeBytes,
    detailLabel:
      record.kind === "image" && imageMeta
        ? `${imageMeta.width} × ${imageMeta.height}`
        : record.kind === "file" && fileMeta?.length === 1 && primaryFile?.extension
          ? primaryFile.extension.toLocaleUpperCase()
          : undefined,
    createdAt: record.createdAtMs,
    favorite: record.isFavorite,
    customTitle: isCustomClipboardTitle(record),
    fileName:
      record.kind === "image" || record.kind === "file"
        ? primaryFile?.name || fileNameFromPath(record.resourcePath) || record.title
        : undefined,
    imageMeta,
    fileMeta,
    resourceMetadata,
    mimeType: resourceMetadata?.mimeType ?? primaryFile?.mimeType,
    previewPath: record.previewPath,
    resourcePath: record.resourcePath,
    contentHash: record.contentHash,
    textContent: record.textContent,
    iconPath: record.iconPath,
    metadataJson: record.metadataJson,
  };
}

export function parseResourceMetadata(
  record: PersistedClipboardItem,
): ResourceMetadata | undefined {
  if (record.kind !== "image" && record.kind !== "file") return undefined;

  const rawMetadata = parseMetadataObject(record.metadataJson);
  const schemaVersion = optionalNumber(rawMetadata.schemaVersion);
  const resourcePath = optionalString(rawMetadata.resourcePath) ?? record.resourcePath ?? undefined;
  const previewPath = optionalString(rawMetadata.previewPath) ?? record.previewPath ?? undefined;
  const storagePath = optionalString(rawMetadata.storagePath) ?? resourcePath;
  const topLevelExtension = normalizeExtension(
    optionalString(rawMetadata.extension) ?? extensionFromPath(resourcePath),
  );
  const topLevelMime =
    optionalString(rawMetadata.mimeType) ??
    mimeTypeFromExtension(topLevelExtension) ??
    (record.kind === "file" ? "application/octet-stream" : undefined);

  if (record.kind === "image") {
    return {
      schemaVersion,
      mimeType: topLevelMime,
      extension: topLevelExtension,
      sizeBytes: optionalNumber(rawMetadata.sizeBytes) ?? record.sizeBytes,
      resourcePath,
      previewPath,
      storagePath,
      originalPath: optionalString(rawMetadata.originalPath),
      contentHash: optionalString(rawMetadata.contentHash) ?? record.contentHash,
      width: optionalNumber(rawMetadata.width),
      height: optionalNumber(rawMetadata.height),
    };
  }

  const rawFiles = Array.isArray(rawMetadata.files) ? rawMetadata.files : [];
  const parsedFiles = rawFiles
    .map((value, index) => parseFileMetadata(value, record, rawFiles.length, index))
    .filter((value): value is ResourceFileMetadata => value !== undefined);
  const fallbackFile =
    parsedFiles.length === 0 && (record.resourcePath || record.title)
      ? createFallbackFileMetadata(record)
      : undefined;
  const files = fallbackFile ? [fallbackFile] : parsedFiles;
  const primaryFile = files[0];

  const explicitMimeType = optionalString(rawMetadata.mimeType);
  const commonMimeType =
    files.length > 1 && files.every((file) => file.mimeType === files[0]?.mimeType)
      ? files[0]?.mimeType
      : undefined;
  const fileMimeType =
    files.length === 1 ? (topLevelMime ?? primaryFile?.mimeType) : commonMimeType;

  return {
    schemaVersion,
    mimeType: explicitMimeType ?? fileMimeType,
    extension: topLevelExtension ?? (files.length === 1 ? primaryFile?.extension : undefined),
    sizeBytes: optionalNumber(rawMetadata.sizeBytes) ?? record.sizeBytes,
    resourcePath: resourcePath ?? primaryFile?.storagePath,
    storagePath: storagePath ?? primaryFile?.storagePath,
    originalPath:
      optionalString(rawMetadata.originalPath) ??
      (files.length === 1 ? primaryFile?.originalPath : undefined),
    contentHash: optionalString(rawMetadata.contentHash) ?? record.contentHash,
    files,
  };
}

function parseFileMetadata(
  value: unknown,
  record: PersistedClipboardItem,
  fileCount: number,
  index: number,
): ResourceFileMetadata | undefined {
  if (!isObject(value)) return undefined;

  const storagePath = optionalString(value.storagePath) ?? optionalString(value.path);
  const originalPath = optionalString(value.originalPath);
  const name =
    optionalString(value.name) ??
    optionalString(value.originalName) ??
    fileNameFromPath(originalPath ?? storagePath ?? null) ??
    (index === 0 ? record.title : undefined);
  if (!name) return undefined;

  const extension = normalizeExtension(
    optionalString(value.extension) ??
      extensionFromPath(name) ??
      extensionFromPath(originalPath) ??
      extensionFromPath(storagePath),
  );
  const sizeBytes =
    optionalNumber(value.sizeBytes) ??
    optionalNumber(value.size) ??
    (fileCount === 1 ? record.sizeBytes : 0);

  return {
    name,
    size: sizeBytes,
    sizeBytes,
    extension,
    mimeType:
      optionalString(value.mimeType) ??
      mimeTypeFromExtension(extension) ??
      "application/octet-stream",
    storagePath: storagePath ?? (index === 0 ? (record.resourcePath ?? undefined) : undefined),
    originalPath,
    contentHash: optionalString(value.contentHash),
    copied: optionalBoolean(value.copied),
    createdAtMs: optionalNumber(value.createdAtMs),
    modifiedAtMs: optionalNumber(value.modifiedAtMs),
    accessedAtMs: optionalNumber(value.accessedAtMs),
    readOnly: optionalBoolean(value.readOnly),
    isDirectory: optionalBoolean(value.isDirectory),
  };
}

function createFallbackFileMetadata(record: PersistedClipboardItem): ResourceFileMetadata {
  const name = fileNameFromPath(record.resourcePath) || record.title;
  const extension = normalizeExtension(extensionFromPath(name));
  return {
    name,
    size: record.sizeBytes,
    sizeBytes: record.sizeBytes,
    extension,
    mimeType: mimeTypeFromExtension(extension) ?? "application/octet-stream",
    storagePath: record.resourcePath ?? undefined,
  };
}

function parseMetadataObject(metadataJson: string | null | undefined): Record<string, unknown> {
  if (!metadataJson) return {};
  try {
    const parsed: unknown = JSON.parse(metadataJson);
    return isObject(parsed) ? parsed : {};
  } catch {
    return {};
  }
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function optionalString(value: unknown): string | undefined {
  return typeof value === "string" && value.trim() ? value : undefined;
}

function optionalNumber(value: unknown): number | undefined {
  return typeof value === "number" && Number.isFinite(value) && value >= 0 ? value : undefined;
}

function optionalBoolean(value: unknown): boolean | undefined {
  return typeof value === "boolean" ? value : undefined;
}

function normalizeExtension(extension: string | undefined): string | undefined {
  const normalized = extension?.replace(/^\.+/, "").trim().toLocaleLowerCase();
  return normalized || undefined;
}

function extensionFromPath(path: string | null | undefined): string | undefined {
  const name = fileNameFromPath(path ?? null);
  if (!name || !name.includes(".")) return undefined;
  return name.split(".").pop();
}

function mimeTypeFromExtension(extension: string | undefined): string | undefined {
  if (!extension) return undefined;
  const mimeTypes: Record<string, string> = {
    txt: "text/plain",
    log: "text/plain",
    md: "text/markdown",
    html: "text/html",
    htm: "text/html",
    css: "text/css",
    csv: "text/csv",
    tsv: "text/tab-separated-values",
    xml: "application/xml",
    json: "application/json",
    yaml: "application/yaml",
    yml: "application/yaml",
    toml: "application/toml",
    js: "text/javascript",
    mjs: "text/javascript",
    ts: "text/typescript",
    tsx: "text/typescript",
    svg: "image/svg+xml",
    png: "image/png",
    jpg: "image/jpeg",
    jpeg: "image/jpeg",
    gif: "image/gif",
    webp: "image/webp",
    bmp: "image/bmp",
    tif: "image/tiff",
    tiff: "image/tiff",
    ico: "image/x-icon",
    pdf: "application/pdf",
    zip: "application/zip",
    rar: "application/vnd.rar",
    "7z": "application/x-7z-compressed",
    gz: "application/gzip",
    tar: "application/x-tar",
    doc: "application/msword",
    docx: "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    xls: "application/vnd.ms-excel",
    xlsx: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ppt: "application/vnd.ms-powerpoint",
    pptx: "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    mp3: "audio/mpeg",
    wav: "audio/wav",
    flac: "audio/flac",
    ogg: "audio/ogg",
    mp4: "video/mp4",
    mov: "video/quicktime",
    avi: "video/x-msvideo",
    webm: "video/webm",
    mkv: "video/x-matroska",
    exe: "application/vnd.microsoft.portable-executable",
    dll: "application/vnd.microsoft.portable-executable",
    sqlite: "application/vnd.sqlite3",
    sqlite3: "application/vnd.sqlite3",
    db: "application/vnd.sqlite3",
  };
  return mimeTypes[extension] ?? "application/octet-stream";
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
