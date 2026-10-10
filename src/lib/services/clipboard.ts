import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { showToast } from "$lib/services/toast";
import { invokeTauri, invokeTauriRequired, isTauriRuntime } from "$lib/services/runtime";
import { logFrontendError, logFrontendMessage } from "$lib/services/log";
import { generalSettings } from "$lib/services/settings";
import { get } from "svelte/store";
import type {
  ClipboardItem,
  HistoryFilterArgs,
  PersistedClipboardItem,
  ResourceFileMetadata,
  ResourceMetadata,
  SortRule,
} from "$lib/types/clipboard";
import { getLocale, resolvePath } from "$lib/i18n";
import { formatBytes } from "$lib/utils/content/format";
import zhCN from "$lib/i18n/locales/zh-CN";
import en from "$lib/i18n/locales/en";

const locales = { "zh-CN": zhCN, en };
const materializationRequests = new Map<string, Promise<ClipboardItem>>();
const contentRequests = new Map<string, Promise<ClipboardItem>>();

/** Coalesce in-flight hydration only; retained bodies belong to the single item store. */
export async function hydrateClipboardItem(item: ClipboardItem): Promise<ClipboardItem> {
  if (item.contentLoaded !== false) return item;
  if (!isTauriRuntime()) throw new Error("Full clipboard content is unavailable");
  const existing = contentRequests.get(item.id);
  if (existing) return existing;
  const request = invoke<PersistedClipboardItem | null>("get_clipboard_item", { id: item.id })
    .then((record) => {
      if (!record) throw new Error("Clipboard record no longer exists");
      return { ...toClipboardItem(record), deleted: item.deleted };
    })
    .finally(() => contentRequests.delete(item.id));
  contentRequests.set(item.id, request);
  return request;
}

export async function writeClipboardText(text: string): Promise<void> {
  if (isTauriRuntime()) {
    try {
      await invoke("mark_self_triggered", { text });
    } catch (error) {
      console.warn("Unable to register text self-trigger", error);
    }
  }
  try {
    await navigator.clipboard.writeText(text);
  } catch (error) {
    // The write failed; drop the pre-write marker so an external copy of the
    // same text inside the suppression window is still captured.
    if (isTauriRuntime()) {
      try {
        await invoke("unmark_self_triggered", { text });
      } catch (unmarkError) {
        console.warn("Unable to clear text self-trigger", unmarkError);
      }
    }
    throw error;
  }
}

/**
 * Reads the current system clipboard text (empty when it holds no text).
 * Tauri reads through the backend platform adapter so the editor context
 * menu's Paste does not depend on the WebView's clipboard-read permission.
 */
export async function readClipboardText(): Promise<string> {
  if (isTauriRuntime()) {
    const text = await invoke<string | null>("read_clipboard_text");
    return text ?? "";
  }
  return await navigator.clipboard.readText();
}

export async function writeClipboardImage(
  blob: Blob,
  resourcePath?: string | null,
  contentHash?: string,
): Promise<void> {
  const shouldMark = isTauriRuntime() && Boolean(resourcePath || contentHash);
  if (shouldMark) {
    try {
      await invoke("mark_self_triggered_image", {
        resourcePath: resourcePath ?? null,
        contentHash: contentHash ?? null,
      });
    } catch (error) {
      console.warn("Unable to register image self-trigger", error);
    }
  }

  try {
    await navigator.clipboard.write([new ClipboardItem({ [blob.type || "image/png"]: blob })]);
  } catch (error) {
    if (shouldMark) {
      try {
        await invoke("unmark_self_triggered_image", {
          resourcePath: resourcePath ?? null,
          contentHash: contentHash ?? null,
        });
      } catch (unmarkError) {
        console.warn("Unable to clear image self-trigger", unmarkError);
      }
    }
    throw error;
  }
}

/** Copies the files behind an image/file record back to the system clipboard
 * as dropped file references (CF_HDROP on Windows). The backend resolves the
 * paths from the record, so no arbitrary paths cross the IPC boundary. */
export async function copyClipboardItemFiles(id: string): Promise<void> {
  await invoke("copy_clipboard_item_files", { id });
}

/**
 * Rejection shape of `copy_clipboard_item_files`.
 *
 * The backend tags the failure so "the file is gone from disk" does not look
 * like "the clipboard was busy": the first is permanent for that record, the
 * second is worth a retry. Anything else (a plain string, or a rejection from a
 * command that has not been converted) is treated as the generic failure.
 */
interface FilesCopyFailure {
  kind?: string;
  message?: string;
}

/** True when the copy failed because the record's files are no longer on disk. */
export function isFilesCopySourceMissing(error: unknown): boolean {
  if (typeof error !== "object" || error === null) return false;
  return (error as FilesCopyFailure).kind === "resource-missing";
}

/** Renders a rejection for the console, whether it is a tag or a plain string. */
function describeInvokeFailure(error: unknown): string {
  if (typeof error === "string") return error;
  if (typeof error === "object" && error !== null) {
    const { kind, message } = error as FilesCopyFailure;
    if (kind) return `${kind}: ${message ?? ""}`.trim();
  }
  return String(error);
}

export async function writeClipboardHtml(
  html: string,
  plainText?: string | null,
  rtf?: string | null,
): Promise<void> {
  const shouldMark = isTauriRuntime() && Boolean(plainText);
  if (shouldMark) {
    try {
      await invoke("mark_self_triggered", { text: plainText });
    } catch (error) {
      console.warn("Unable to register HTML self-trigger", error);
    }
  }

  const payload: Record<string, Blob> = {
    "text/html": new Blob([html], { type: "text/html" }),
  };
  if (plainText) {
    payload["text/plain"] = new Blob([plainText], { type: "text/plain" });
  }
  if (rtf) {
    payload["text/rtf"] = new Blob([rtf], { type: "text/rtf" });
  }
  try {
    await navigator.clipboard.write([new ClipboardItem(payload)]);
  } catch (error) {
    if (shouldMark && plainText) {
      try {
        await invoke("unmark_self_triggered", { text: plainText });
      } catch (unmarkError) {
        console.warn("Unable to clear HTML self-trigger", unmarkError);
      }
    }
    throw error;
  }
}

export async function loadClipboardHistory(
  limit = 100,
  offset = 0,
  filter: HistoryFilterArgs = {},
): Promise<ClipboardItem[] | null> {
  if (!isTauriRuntime()) return null;

  const records =
    (await invokeTauri<PersistedClipboardItem[]>("list_clipboard_items", {
      limit,
      offset,
      filter,
    })) ?? [];

  return records.map(toClipboardItem);
}

/** Loads the persisted recycle-bin page. Deleted records are marked locally
 * because the regular ClipboardItem payload intentionally represents active
 * history only. */
export async function loadDeletedClipboardHistory(
  limit = 1000,
  offset = 0,
): Promise<ClipboardItem[] | null> {
  if (!isTauriRuntime()) return null;

  const records =
    (await invokeTauri<PersistedClipboardItem[]>("list_deleted_clipboard_items", {
      limit,
      offset,
    })) ?? [];

  return records.map((record) => ({ ...toClipboardItem(record), deleted: true }));
}

/** Ensures every remotely referenced image/file/icon for one record has a
 * verified local path. Concurrent callers share one in-flight backend request
 * so copy, preview, fullscreen and save never download the same blob twice. */
export async function materializeClipboardItem(item: ClipboardItem): Promise<ClipboardItem> {
  item = await hydrateClipboardItem(item);
  if (!isTauriRuntime() || (item.kind !== "image" && item.kind !== "file")) return item;

  const existing = materializationRequests.get(item.id);
  if (existing) return existing;

  const request = invoke<PersistedClipboardItem>("materialize_clipboard_item", { id: item.id })
    .then(toClipboardItem)
    .finally(() => materializationRequests.delete(item.id));
  materializationRequests.set(item.id, request);
  return request;
}

export interface SearchPage {
  items: ClipboardItem[];
  totalCount: number;
  truncated: boolean;
}

interface PersistedSearchPage {
  items: PersistedClipboardItem[];
  totalCount: number;
  truncated: boolean;
}

export async function searchClipboardHistory(
  query: string,
  limit = 100,
  offset = 0,
  sortRules?: SortRule[],
  filter?: HistoryFilterArgs,
): Promise<SearchPage | null> {
  if (!isTauriRuntime()) return null;

  const page = await invokeTauri<PersistedSearchPage>("search_clipboard_items", {
    query,
    limit,
    offset,
    sortRules,
    filter,
  });
  if (!page) return null;

  return {
    items: (page.items ?? []).map(toClipboardItem),
    totalCount: page.totalCount ?? 0,
    truncated: page.truncated ?? false,
  };
}

export async function persistFavorite(id: string, isFavorite: boolean): Promise<boolean | null> {
  return invokeTauri<boolean>("set_clipboard_item_favorite", { id, isFavorite });
}

export async function persistDelete(id: string): Promise<boolean | null> {
  return invokeTauri<boolean>("soft_delete_clipboard_item", { id });
}

export async function persistHardDelete(id: string): Promise<boolean | null> {
  return invokeTauri<boolean>("delete_clipboard_item", { id });
}

export async function persistRestore(id: string): Promise<boolean | null> {
  return invokeTauri<boolean>("restore_clipboard_item", { id });
}

export async function persistBatchRestore(ids: string[]): Promise<boolean | null> {
  return invokeTauri<boolean>("batch_restore_clipboard_items", { ids });
}

export async function persistPermanentDelete(id: string): Promise<boolean | null> {
  return invokeTauri<boolean>("permanently_delete_clipboard_item", { id });
}

export async function persistBatchPermanentDelete(ids: string[]): Promise<boolean | null> {
  return invokeTauri<boolean>("batch_permanently_delete_clipboard_items", { ids });
}

export async function persistBatchFavorite(
  ids: string[],
  isFavorite: boolean,
): Promise<boolean | null> {
  return invokeTauri<boolean>("batch_set_favorite", { ids, isFavorite });
}

export async function persistBatchDelete(ids: string[]): Promise<boolean | null> {
  return invokeTauri<boolean>("batch_delete_clipboard_items", { ids });
}

export async function persistTags(id: string, tags: string[]): Promise<boolean | null> {
  return invokeTauri<boolean>("set_clipboard_item_tags", { id, tags });
}

/** Records that a record was reused (copied/pasted out from history) so the
 * optional `LastUsedAt` search sort reflects successful clipboard writes. */
export async function persistLastUsed(id: string): Promise<boolean | null> {
  return invokeTauri<boolean>("set_clipboard_item_last_used", { id });
}

export interface TagInfo {
  name: string;
  count: number;
  color: string;
}

export async function listAllTags(): Promise<TagInfo[] | null> {
  return invokeTauri<TagInfo[]>("list_all_tags");
}

export async function renameTag(old: string, newName: string): Promise<number | null> {
  return invokeTauri<number>("rename_tag", { old, new: newName });
}

export async function deleteTag(name: string): Promise<number | null> {
  return invokeTauri<number>("delete_tag", { name });
}

export async function setTagColor(name: string, color: string): Promise<boolean | null> {
  return invokeTauri<boolean>("set_tag_color", { name, color });
}

export interface AutoTagRule {
  pattern: string;
  tag: string;
  sourceApp?: string;
  kind?: ClipboardItem["kind"] | null;
}

export interface AutoTagHistoryPreview {
  matchedCount: number;
  changedCount: number;
  samples: { id: string; title: string; tags: string[] }[];
}
export async function previewAutoTagRules(
  rules: AutoTagRule[],
  text: string,
  sourceApp: string,
  kind: ClipboardItem["kind"],
) {
  return invokeTauriRequired<string[]>(
    "preview_auto_tag_rules",
    { rules, text, sourceApp, kind },
    "Auto-tag preview requires the desktop app",
  );
}
export async function previewAutoTagHistory(rules: AutoTagRule[]) {
  return invokeTauriRequired<AutoTagHistoryPreview>(
    "preview_auto_tag_history",
    { rules },
    "Auto-tag preview requires the desktop app",
  );
}
export async function applyAutoTagHistory(rules: AutoTagRule[]) {
  return invokeTauriRequired<AutoTagHistoryPreview>(
    "apply_auto_tag_history",
    { rules },
    "Auto-tag application requires the desktop app",
  );
}

export async function getAutoTagRules(): Promise<AutoTagRule[] | null> {
  return invokeTauri<AutoTagRule[]>("get_auto_tag_rules");
}

export async function setAutoTagRules(rules: AutoTagRule[]): Promise<AutoTagRule[] | null> {
  return invokeTauriRequired<AutoTagRule[]>(
    "set_auto_tag_rules",
    { rules },
    "Auto-tag rules are only available in the desktop app",
  );
}

export async function listSourceApplications(): Promise<string[] | null> {
  return invokeTauri<string[]>("list_source_applications");
}

export interface QuickAction {
  label: string;
  actionType: "open" | "copy" | "viewDate";
  payload: string;
  kind?: "url" | "email" | "phone" | "date" | "color" | "copy";
}

export async function detectContentActions(text: string): Promise<QuickAction[] | null> {
  return invokeTauri<QuickAction[]>("detect_content_actions", { text });
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

export function getDisplayTitle(text: string): string {
  const match = /[^\r\n]+/.exec(text);
  return match ? match[0].trim() : "";
}

export interface TextEditPatch {
  /** Text-like records carry a generated title, preview and size. */
  isText: boolean;
  /** Image/file records rename in place and keep their stored text. */
  isMedia: boolean;
  newTitle: string;
  newTextContent: string | null;
  newPreview: string;
  newSizeBytes: number;
  newSizeLabel: string;
}

/**
 * Derives the optimistic patch for an in-place text edit. Pure so the title /
 * preview / size rules can be unit-tested without the route's state; the
 * caller still owns persistence and the four-copy fan-out.
 */
export function deriveTextEditPatch(item: ClipboardItem, content: string): TextEditPatch {
  const isText = item.kind === "text" || item.kind === "link";
  const newTitle = isText
    ? item.customTitle
      ? item.title
      : generatedClipboardTitle(content)
    : content;
  // Mirror the load-time preview rule in buildPreview so the card never shows
  // a preview that silently changes or disappears on the next reload: for
  // text records the preview is the second line while the content differs
  // from the (generated) title, and empty otherwise. The previous in-place
  // rule (stale preview, or content.slice(200) for long content) disagreed
  // with that reload-time value.
  const contentLines = isText ? content.split("\n") : [];
  const newPreview = !isText
    ? (item.preview ?? "")
    : content && content !== newTitle && contentLines.length > 1
      ? contentLines[1]
      : "";
  return {
    isText,
    isMedia: item.kind === "image" || item.kind === "file",
    newTitle,
    newTextContent: isText ? content : (item.textContent ?? null),
    newPreview,
    newSizeBytes: new TextEncoder().encode(content).byteLength,
    newSizeLabel: formatTextLength(content.length),
  };
}

export interface CopyItemHooks {
  /** Reorder callback (the main list pins the copied entry to the top). */
  moveToTop?: (id: string) => void;
  /** Status-line callback; the float panel has no status line. */
  onstatus?: (message: string) => void;
}

async function recordSuccessfulUsage(
  item: ClipboardItem,
  hooks: { moveToTop?: (id: string) => void },
): Promise<void> {
  try {
    const updated = await persistLastUsed(item.id);
    if ((updated || !isTauriRuntime()) && get(generalSettings).pinCopiedToTop) {
      hooks.moveToTop?.(item.id);
    }
  } catch (error) {
    // The OS write succeeded. A metadata failure must not report copy failure
    // or move the UI ahead of the persisted order.
    console.error("Unable to persist last-used item", error);
  }
}

/**
 * Copies one history entry back to the system clipboard, shared by the main
 * list and the float panel. Materializes remote image/file records first,
 * registers self-trigger marks through the write helpers, and reports
 * through toasts (plus the optional status hook).
 */
export async function copyClipboardItem(
  item: ClipboardItem,
  hooks: CopyItemHooks = {},
): Promise<void> {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const t = (path: string, params?: Record<string, string | number>) =>
    resolvePath(messages, path, params);

  if (item.kind === "image" || item.kind === "file") {
    try {
      item = await materializeClipboardItem(item);
    } catch (error) {
      logFrontendError("materialize clipboard item for copy", error);
      showToast(t("toast.copyFailed"), "error");
      return;
    }
  }

  if (isTauriRuntime()) {
    try {
      const updated = await invoke<boolean>("copy_clipboard_item", { id: item.id });
      if (updated && get(generalSettings).pinCopiedToTop) hooks.moveToTop?.(item.id);
      hooks.onstatus?.(t("app.copiedItem", { title: getDisplayTitle(item.title) }));
      showToast(t("toast.copySuccess"), "success");
    } catch (error) {
      logFrontendMessage("error", `Unable to copy item: ${describeInvokeFailure(error)}`);
      showToast(
        t(isFilesCopySourceMissing(error) ? "toast.copySourceMissing" : "toast.copyFailed"),
        "error",
      );
    }
    return;
  }

  try {
    item = await hydrateClipboardItem(item);
  } catch (error) {
    logFrontendError("hydrate clipboard item for copy", error);
    showToast(t("toast.copyFailed"), "error");
    return;
  }

  if (item.kind === "image" && item.resourcePath) {
    try {
      const src = convertFileSrc(item.resourcePath.replace(/\\/g, "/"));
      const response = await fetch(src);
      const blob = await response.blob();
      await writeClipboardImage(blob, item.resourcePath, item.contentHash);
      await recordSuccessfulUsage(item, hooks);
      hooks.onstatus?.(t("app.copiedItem", { title: getDisplayTitle(item.title) }));
      showToast(t("toast.copySuccess"), "success");
    } catch (error) {
      logFrontendError("copy clipboard image", error);
      showToast(t("toast.copyFailed"), "error");
    }
    return;
  }

  if (item.kind === "file") {
    if (item.textContent && item.textContent.startsWith("[")) {
      try {
        const paths = JSON.parse(item.textContent) as string[];
        // Single-file records carry a one-element list; only a multi-file
        // check here would leave them to fall through silently below.
        if (Array.isArray(paths) && paths.length > 0) {
          const joined = paths.join("\n");
          if (joined.trim().length > 0) {
            await writeClipboardText(joined);
            await recordSuccessfulUsage(item, hooks);
            hooks.onstatus?.(t("app.copiedItem", { title: getDisplayTitle(item.title) }));
            showToast(t("toast.copySuccess"), "success");
            return;
          }
        }
      } catch {
        /* ignore */
      }
    }
    if (item.resourcePath) {
      try {
        await writeClipboardText(item.resourcePath);
        await recordSuccessfulUsage(item, hooks);
        hooks.onstatus?.(
          t("app.copiedItem", { title: item.fileName || getDisplayTitle(item.title) }),
        );
        showToast(t("toast.copySuccess"), "success");
      } catch (error) {
        logFrontendError("copy clipboard file path", error);
        showToast(t("toast.copyFailed"), "error");
      }
    } else {
      // Neither a path list nor a resource path: report instead of
      // returning silently so the click has visible feedback.
      showToast(t("toast.copyFailed"), "error");
    }
    return;
  }

  await writeClipboardText(item.textContent || item.title)
    .then(async () => {
      await recordSuccessfulUsage(item, hooks);
      hooks.onstatus?.(t("app.copiedItem", { title: getDisplayTitle(item.title) }));
      showToast(t("toast.copySuccess"), "success");
    })
    .catch((error) => {
      // The toast alone left "copy failed" undiagnosable: the most common cause
      // is Windows clipboard contention (another process holding it open), and
      // without this line there was no way to tell that from a missing resource.
      logFrontendError("copy clipboard text", error);
      showToast(t("toast.copyFailed"), "error");
    });
}

export function getDisplayRemainingLines(text: string): string {
  const match = /[^\r\n]+/.exec(text);
  if (!match) return "";

  // 切出第一行后面的部分，直接调用原生 trimStart() 剥离开头所有不可见字符
  return text.slice(match.index + match[0].length).trimStart();
}

/**
 * Copies the local file paths behind an image/file record as text (joined by
 * newlines). Files prefer the recorded original path so the user pastes the
 * path they recognize; the managed storage path is the fallback.
 */
export async function copyClipboardPath(
  item: ClipboardItem,
  hooks: CopyItemHooks = {},
): Promise<void> {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const t = (path: string, params?: Record<string, string | number>) =>
    resolvePath(messages, path, params);

  let media = item;
  if (media.kind === "image" || media.kind === "file") {
    try {
      media = await materializeClipboardItem(media);
    } catch (error) {
      logFrontendError("materialize clipboard item for path copy", error);
      showToast(t("toast.copyFailed"), "error");
      return;
    }
  }

  const lines = clipboardPathLines(media);
  if (lines.length === 0) {
    showToast(t("toast.copyFailed"), "error");
    return;
  }

  try {
    await writeClipboardText(lines.join("\n"));
    hooks.onstatus?.(t("app.copiedItem", { title: getDisplayTitle(media.title) }));
    showToast(t("toast.copySuccess"), "success");
  } catch {
    showToast(t("toast.copyFailed"), "error");
  }
}

/**
 * The "copy path" text lines for an image/file record. Images use the recorded
 * file path; files prefer the per-file original path (the path the user
 * recognizes) and fall back to the recorded resource path. Always empty for
 * unsupported kinds or records without a resolvable file path.
 */
export function clipboardPathLines(media: ClipboardItem): string[] {
  const lines: string[] = [];
  if (media.kind === "image") {
    if (media.resourcePath) lines.push(media.resourcePath);
  } else if (media.kind === "file") {
    const files = media.fileMeta ?? [];
    if (files.length > 0) {
      for (const file of files) {
        const path = file.originalPath || file.storagePath;
        if (path) lines.push(path);
      }
    } else if (media.resourcePath) {
      lines.push(media.resourcePath);
    }
  }
  return lines;
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

export interface TextTransformResult {
  input: string;
  operation: string;
  result: string;
}

export type PasteMode = "plain" | "format" | "clean" | "auto";

export interface PasteItemHooks {
  /** Reorder callback (the main list pins the pasted entry to the top). */
  moveToTop?: (id: string) => void;
}

interface PasteMessageKeys {
  paste: string;
  copy: string;
  failed: string;
}

async function cleanTextIfEnabled(text: string): Promise<string> {
  if (!get(generalSettings).pasteCleaningEnabled) return text;
  try {
    const transform = await invokeTauri<TextTransformResult>("transform_text", {
      operation: "cleanPaste",
      input: text,
    });
    return transform?.result ?? text;
  } catch (error) {
    console.error("Unable to clean text before paste", error);
    return text;
  }
}

async function pasteToPreviousApp(
  item: ClipboardItem,
  keys: PasteMessageKeys,
  // The writer can return replacement keys when it produced a different
  // representation than the requested mode (e.g. a cleaned plain-text
  // downgrade of an explicit format paste), so the toast stays truthful.
  write: () => Promise<PasteMessageKeys | void>,
  hooks: PasteItemHooks = {},
): Promise<void> {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const t = (path: string, params?: Record<string, string | number>) =>
    resolvePath(messages, path, params);

  let resolvedKeys = keys;
  try {
    resolvedKeys = (await write()) ?? keys;
  } catch (error) {
    // Same split as the copy path: a record whose file is gone must not read as
    // a transient paste failure.
    console.error("Unable to prepare clipboard content for paste", describeInvokeFailure(error));
    showToast(
      t(isFilesCopySourceMissing(error) ? "toast.copySourceMissing" : resolvedKeys.failed),
      "error",
    );
    return;
  }
  await recordSuccessfulUsage(item, hooks);

  if (!isTauriRuntime()) {
    showToast(t(resolvedKeys.copy), "success");
    return;
  }

  try {
    const pasted = await invokeTauri<boolean>("paste_to_previous_application");
    showToast(t(pasted ? resolvedKeys.paste : resolvedKeys.copy), pasted ? "success" : "info");
  } catch (error) {
    console.error("Unable to restore the previous application and paste", error);
    showToast(t(resolvedKeys.failed), "error");
  }
}

/**
 * Pastes one history entry into the previous application (or copies it
 * outside Tauri), shared by the main list, the detail panel, and the float
 * panel. `auto` picks the richest available representation by kind.
 */
export async function pasteClipboardItem(
  item: ClipboardItem,
  mode: PasteMode = "auto",
  hooks: PasteItemHooks = {},
): Promise<void> {
  const locale = getLocale();
  const messages = locales[locale] ?? locales.en;
  const t = (path: string, params?: Record<string, string | number>) =>
    resolvePath(messages, path, params);

  try {
    item = await hydrateClipboardItem(item);
  } catch (error) {
    logFrontendError("hydrate clipboard item for paste", error);
    showToast(t("toast.copyFailed"), "error");
    return;
  }

  if (mode === "plain") {
    const text = item.textContent || item.title;
    await pasteToPreviousApp(
      item,
      {
        paste: "toast.plainPasteSuccess",
        copy: "toast.plainCopySuccess",
        failed: "toast.plainPasteFailed",
      },
      async () => {
        const cleaned = await cleanTextIfEnabled(text);
        await writeClipboardText(cleaned);
      },
      hooks,
    );
    return;
  }

  if (mode === "clean") {
    const text = item.textContent || item.title;
    await pasteToPreviousApp(
      item,
      {
        paste: "toast.cleanPasteSuccess",
        copy: "toast.cleanCopySuccess",
        failed: "toast.cleanPasteFailed",
      },
      async () => {
        const transform = await invokeTauri<TextTransformResult>("transform_text", {
          operation: "cleanPaste",
          input: text,
        });
        await writeClipboardText(transform?.result ?? text);
      },
      hooks,
    );
    return;
  }

  if (mode === "format") {
    if (!item.htmlContent) return;
    const htmlContent = item.htmlContent;
    const plainText = item.textContent || undefined;
    await pasteToPreviousApp(
      item,
      {
        paste: "toast.formatPasteSuccess",
        copy: "toast.formatCopySuccess",
        failed: "toast.formatPasteFailed",
      },
      async () => {
        if (plainText && get(generalSettings).pasteCleaningEnabled) {
          const cleaned = await cleanTextIfEnabled(plainText);
          if (cleaned !== plainText) {
            await writeClipboardText(cleaned);
            // Cleaning cannot be applied inside markup, so an explicit
            // format paste degrades to cleaned plain text here. Report the
            // real representation instead of claiming the format was kept.
            return {
              paste: "toast.formatPasteCleanedSuccess",
              copy: "toast.formatCopyCleanedSuccess",
              failed: "toast.formatPasteFailed",
            };
          }
        }
        await writeClipboardHtml(htmlContent, plainText, item.rtfContent);
      },
      hooks,
    );
    return;
  }

  // auto: richest representation by kind.
  if (item.kind === "text" || item.kind === "link") {
    if (item.htmlContent) {
      await pasteClipboardItem(item, "format", hooks);
    } else {
      await pasteClipboardItem(item, "plain", hooks);
    }
    return;
  }

  if (item.kind === "image" || item.kind === "file") {
    let media = item;
    try {
      media = await materializeClipboardItem(item);
    } catch (error) {
      console.error("Unable to materialize media for paste", error);
      showToast(
        t(item.kind === "image" ? "toast.imagePasteFailed" : "toast.filePasteFailed"),
        "error",
      );
      return;
    }
    if (item.kind === "image") {
      await pasteToPreviousApp(
        media,
        {
          paste: "toast.imagePasteSuccess",
          copy: "toast.imageCopySuccess",
          failed: "toast.imagePasteFailed",
        },
        async () => {
          const src = convertFileSrc((media.resourcePath ?? "").replace(/\\/g, "/"));
          const response = await fetch(src);
          const blob = await response.blob();
          await writeClipboardImage(blob, media.resourcePath, media.contentHash);
        },
        hooks,
      );
      return;
    }
    await pasteToPreviousApp(
      media,
      {
        paste: "toast.filePasteSuccess",
        copy: "toast.fileCopySuccess",
        failed: "toast.filePasteFailed",
      },
      async () => {
        // Files must reach the OS clipboard as file drops (CF_HDROP),
        // otherwise a later Ctrl+V only pastes a path string and the
        // target app cannot accept it as a file.
        if (isTauriRuntime()) {
          await copyClipboardItemFiles(media.id);
          return;
        }
        if (media.resourcePath) {
          await writeClipboardText(media.resourcePath);
        }
      },
      hooks,
    );
  }
}
