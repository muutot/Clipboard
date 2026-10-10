// Persisted-record mapping: turns backend rows into domain `ClipboardItem`
// values plus the display metadata (resource metadata, preview, size label,
// source tone) they carry. The IPC wrappers themselves stay in
// `$lib/services/clipboard`.

import { getLocale, resolvePath } from "$lib/i18n";
import zhCN from "$lib/i18n/locales/zh-CN";
import en from "$lib/i18n/locales/en";
import { formatBytes } from "$lib/utils/content/format";
import { generatedClipboardTitle, isCustomClipboardTitle } from "$lib/utils/content/display-text";
import type {
  ClipboardItem,
  PersistedClipboardItem,
  ResourceFileMetadata,
  ResourceMetadata,
} from "$lib/types/clipboard";

const locales = { "zh-CN": zhCN, en };

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
  const preview = buildPreview(record, fileLabel, imageLabel);

  return {
    id: record.id,
    contentLoaded: record.contentLoaded !== false,
    hasHtml: record.htmlContent != null,
    kind: record.kind,
    title: record.title,
    preview,
    sourceApp,
    sourceTone: sourceTone(sourceApp, locale),
    sizeLabel:
      record.contentLoaded === false ? formatBytes(record.sizeBytes) : formatSizeSimple(record),
    sizeBytes: record.sizeBytes,
    createdAt: record.createdAtMs,
    lastUsedAtMs: record.lastUsedAtMs,
    favorite: record.isFavorite,
    customTitle: isCustomClipboardTitle(record),
    fileName:
      record.kind === "image" || record.kind === "file"
        ? primaryFile?.name || fileNameFromPath(record.resourcePath) || record.title
        : undefined,
    searchableText: [record.title, preview, record.textContent, record.sourceApp]
      .filter(Boolean)
      .join(" ")
      .toLocaleLowerCase(),
    imageMeta,
    fileMeta,
    resourceMetadata,
    mimeType: resourceMetadata?.mimeType ?? primaryFile?.mimeType,
    previewPath: record.previewPath,
    resourcePath: record.resourcePath,
    contentHash: record.contentHash,
    textContent: record.textContent,
    htmlContent: record.htmlContent,
    rtfContent: record.rtfContent,
    iconPath: record.iconPath,
    metadataJson: record.metadataJson,
    tags: parseTags(record.metadataJson),
  };
}

function parseTags(metadataJson: string | null | undefined): string[] {
  const metadata = parseMetadataObject(metadataJson);
  if (!Array.isArray(metadata.tags)) return [];
  return metadata.tags
    .filter((value): value is string => typeof value === "string")
    .map((value) => value.trim())
    .filter(Boolean);
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
  return MIME_TYPES[extension] ?? "application/octet-stream";
}

const MIME_TYPES: Record<string, string> = {
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
  return formatBytes(record.sizeBytes);
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

/**
 * Dot colors for the source-app tone shown on each card.
 *
 * These are content semantics, not theme chrome: the tone encodes which family
 * an app belongs to, so the hues stay fixed instead of following the theme —
 * recoloring them per theme would make "red app" mean something different in
 * light and dark. `neutral` is the exception because it means "no signal" and
 * should recede with the rest of the muted text. Reviewed and kept as a local
 * exception; see `references/niche_ui_style.md`.
 */
export const SOURCE_TONE_COLORS: Record<ClipboardItem["sourceTone"], string> = {
  neutral: "var(--text-muted)",
  red: "#ff4655",
  blue: "#66bde1",
  violet: "#746dff",
};
