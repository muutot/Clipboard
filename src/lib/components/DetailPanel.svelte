<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CodePreview from "$lib/components/CodePreview.svelte";
  import MarkdownPreview from "$lib/components/MarkdownPreview.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatRelativeTime } from "$lib/utils/time";
  import { convertFileSrc, invoke } from "@tauri-apps/api/core";
  import { isTauriRuntime } from "$lib/services/runtime";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  function assetUrl(filePath: string | null | undefined): string | undefined {
    if (!filePath) return undefined;
    if (!isTauriRuntime()) return undefined;
    try {
      return convertFileSrc(filePath.replace(/\\/g, "/"));
    } catch {
      return undefined;
    }
  }

  interface Props {
    item: ClipboardItem | null;
    onclose: () => void;
    oncopy: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void;
    onplainpaste: (id: string) => void;
    onformatpaste: (id: string) => void;
  }

  let { item, onclose, oncopy, onedit, onsaveedit, onplainpaste, onformatpaste }: Props = $props();

  let activeTab = $state<"preview" | "details" | "ocr">("preview");
  let editing = $state(false);
  let editContent = $state("");

  $effect(() => {
    if (item?.kind !== "image" || !isTauriRuntime()) return;
    
    const poll = () => {
      if (item.ocrStatus === "completed") return;
      invoke<{ fullText: string; status: string }>("get_clipboard_item_ocr", { id: item.id })
        .then(result => {
          if (result) {
            item.ocrStatus = result.status === "completed" ? "completed" : "pending";
            if (result.fullText) item.ocrText = result.fullText;
          }
        })
        .catch(() => { item.ocrStatus = "none"; });
    };
    
    poll();
    const interval = setInterval(poll, 2000);
    return () => clearInterval(interval);
  });

  const emails = $derived(
    item ? [...new Set(([item.title, item.preview].filter(Boolean).join(" ").match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g) ?? []))] : [],
  );
  const urls = $derived(
    item ? [...new Set(([item.title, item.preview].filter(Boolean).join(" ").match(/https?:\/\/[^\s)]+/g) ?? []))] : [],
  );
  const phones = $derived(
    item ? [...new Set(([item.title, item.preview].filter(Boolean).join(" ").match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g) ?? []))] : [],
  );
  const colors = $derived(
    item ? [...new Set(([item.title, item.preview].filter(Boolean).join(" ").match(/#(?:[0-9a-fA-F]{3}){1,2}\b/g) ?? []))] : [],
  );

  const hasSpecialMarkers = $derived(
    emails.length > 0 || urls.length > 0 || phones.length > 0 || colors.length > 0,
  );

  const isCode = $derived(item ? detectCodeLanguage(item.title) !== null : false);
  const isMarkdown = $derived(item ? /^#{1,6}\s|^>\s|^-\s|^\*\*|^\`\`\`|^\[.+\]\(.+\)/m.test(item.title) : false);

  function detectCodeLanguage(text: string): string | null {
    const patterns: [RegExp, string][] = [
      [new RegExp("^(import|export)\\s|interface\\s|type\\s\\w+\\s*=\\s*\\{|const\\s\\w+:\\s*\\w+|function\\s\\w+\\(|\\.\\.\\.\\w+|useState|useEffect|async\\s+function", "ms"), "TypeScript"],
      [new RegExp("^<\\w+[^>]*>|<\\/\\w+>|className=|useState|useEffect|props\\.", "m"), "JSX"],
      [new RegExp("^use\\s|^fn\\s|let\\s+mut|struct\\s|impl\\s|^\\s*pub\\s|^\\s*mod\\s", "m"), "Rust"],
      [new RegExp("^def\\s|^import\\s\\w|^\\s*class\\s|^\\s*from\\s|print\\(|lambda\\s", "m"), "Python"],
      [new RegExp('^\\s*[{\\[]\\s*$|"[^"]*"\\s*:|^\\s*"|function\\s*\\(|require\\(|module\\.exports', "m"), "JSON"],
      [new RegExp("^<!DOCTYPE|<html|<head|<body|<div|<span|\\.class\\s*\\{|#id\\s*\\{", "m"), "HTML"],
      [new RegExp("^SELECT\\s|^INSERT\\s|^UPDATE\\s|^DELETE\\s|^CREATE\\s|^\\s*FROM\\s|^\\s*WHERE\\s", "mi"), "SQL"],
      [new RegExp("^#!/|^\\s*(echo|export|cd|ls|grep|mkdir|sudo|apt|npm|yarn|git)\\s", "m"), "Shell"],
      [new RegExp("^\\.\\w+\\s*\\{|^\\s*color:|^\\s*margin:|^\\s*padding:|@media|@keyframes", "m"), "CSS"],
      [new RegExp("^(function|var|const|let)\\s|^\\s*console\\.|document\\.|window\\.|require\\(", "m"), "JavaScript"],
    ];

    for (const [regex, lang] of patterns) {
      if (regex.test(text)) return lang;
    }
    return null;
  }

  function formatDateTime(ts: number): string {
    return new Date(ts).toLocaleString();
  }

  function getKindLabel(kind: string): string {
    const map: Record<string, string> = {
      text: _t("filter.text"),
      link: _t("filter.link"),
      image: _t("filter.image"),
      file: _t("filter.file"),
    };
    return map[kind] ?? kind;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (item && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose();
    }
  }
</script>

<svelte:window onkeydown={handleKeydown} />

{#if item}
  <div class="detail-backdrop" onclick={onclose} aria-hidden="true"></div>
  <div class="detail-panel" role="dialog" aria-modal="true" aria-label={_t("detail.title")}>
    <div class="detail-header">
      <button class="back-btn" type="button" onclick={onclose} aria-label={_t("detail.back")}>
        <AppIcon name="chevron-left" size={18} strokeWidth={2} />
      </button>
      <div class="header-info">
        <span class="header-kind">{getKindLabel(item.kind)}</span>
        <span class="header-title">{item.title.split("\n")[0]}</span>
      </div>
    </div>

    <nav class="detail-tabs" aria-label="详情选项卡">
      <button class:active={activeTab === "preview"} type="button" onclick={() => (activeTab = "preview")}>
        {_t("detail.preview")}
      </button>
      <button class:active={activeTab === "details"} type="button" onclick={() => (activeTab = "details")}>
        {_t("detail.details")}
      </button>
      <button class:active={activeTab === "ocr"} type="button" onclick={() => (activeTab = "ocr")}>
        {_t("detail.ocr")}
      </button>
    </nav>

    <div class="detail-body">
      {#if activeTab === "preview"}
        <div class="preview-section">
          {#if item.kind === "image"}
            <div class="image-full-preview">
              {#if assetUrl(item.previewPath || item.resourcePath)}
                <img
                  src={assetUrl(item.previewPath || item.resourcePath)}
                  alt={item.preview || item.title}
                />
              {:else}
                <div class="image-placeholder">
                  <AppIcon name="image" size={48} strokeWidth={1.5} />
                  {#if item.imageMeta}
                    <span class="image-meta">{item.imageMeta.width} × {item.imageMeta.height}</span>
                  {/if}
                </div>
              {/if}
            </div>
          {:else if item.kind === "file"}
            <div class="file-full-preview">
              <AppIcon name="file" size={48} strokeWidth={1.5} />
              <strong>{item.fileName ?? item.title}</strong>
              <span>{item.preview}</span>
            </div>
          {:else if isCode && !isMarkdown}
            <CodePreview content={item.title} />
          {:else if isMarkdown}
            <MarkdownPreview content={item.title} />
          {:else}
            {#if editing}
              <div class="edit-area">
                <textarea bind:value={editContent} rows={8}></textarea>
              </div>
              <div class="edit-actions">
                <button type="button" class="edit-save" onclick={() => {
                  if (item) onsaveedit(item.id, editContent);
                  item!.title = editContent.split('\n')[0];
                  item!.textContent = editContent;
                  editing = false;
                }}>
                  <AppIcon name="check" size={14} strokeWidth={2.5} /> {_t("edit.save")}
                </button>
                <button type="button" class="edit-cancel" onclick={() => (editing = false)}>
                  <AppIcon name="x" size={14} strokeWidth={2.5} /> {_t("edit.cancel")}
                </button>
              </div>
            {:else}
              <pre class="content-full">{item.title}</pre>
            {/if}
          {/if}
        </div>

        <div class="detail-actions">
          <button type="button" onclick={() => oncopy(item.id)}>
            <AppIcon name="copy" size={15} /> {_t("card.copy")}
          </button>
          {#if (item.kind === "text" || item.kind === "link") && !editing}
            <button type="button" onclick={() => {
              editContent = item.textContent || item.title;
              editing = true;
            }}>
              <AppIcon name="edit" size={15} /> {_t("edit.edit")}
            </button>
          {/if}
          <button type="button" onclick={() => onplainpaste(item.id)}>
            <AppIcon name="type" size={15} /> {_t("paste.plainText")}
          </button>
          <button type="button" onclick={() => onformatpaste(item.id)}>
            <AppIcon name="copy-plus" size={15} /> {_t("paste.withFormat")}
          </button>
        </div>

      {:else if activeTab === "details"}
        <div class="details-section">
          <dl class="detail-list">
            <div class="detail-row">
              <dt><AppIcon name="info" size={14} /> {_t("detail.sourceApp")}</dt>
              <dd>{item.sourceApp}</dd>
            </div>
            <div class="detail-row">
              <dt><AppIcon name="file" size={14} /> {_t("detail.contentType")}</dt>
              <dd>{getKindLabel(item.kind)}</dd>
            </div>
            <div class="detail-row">
              <dt><AppIcon name="clock" size={14} /> {_t("detail.copyTime")}</dt>
              <dd>{formatDateTime(item.createdAt)}</dd>
            </div>
            <div class="detail-row">
              <dt>{_t("detail.size")}</dt>
              <dd>{item.sizeLabel}</dd>
            </div>
            {#if item.detailLabel}
              <div class="detail-row">
                <dt><AppIcon name="image" size={14} /> {_t("detail.fileInfo")}</dt>
                <dd>{item.detailLabel}</dd>
              </div>
            {/if}
            {#if item.mimeType}
              <div class="detail-row">
                <dt><AppIcon name="mime" size={14} /> {_t("detail.mimeInfo")}</dt>
                <dd><code>{item.mimeType}</code></dd>
              </div>
            {/if}
            {#if item.fileName && item.kind === "file"}
              <div class="detail-row">
                <dt>{_t("detail.fileInfo")}</dt>
                <dd>{item.fileName}</dd>
              </div>
            {/if}
            {#if item.ocrStatus}
              <div class="detail-row">
                <dt>{_t("detail.ocrStatus")}</dt>
                <dd class="ocr-badge" class:ocr-completed={item.ocrStatus === "completed"}
                  class:ocr-pending={item.ocrStatus === "pending"}>
                  {item.ocrStatus === "completed" ? _t("detail.completed") : item.ocrStatus === "pending" ? _t("detail.pending") : item.ocrStatus}
                </dd>
              </div>
            {/if}
          </dl>

          {#if hasSpecialMarkers}
            <div class="special-section">
              <strong class="special-title">{_t("detail.specialMarkers")}</strong>
              <div class="markers-list">
                {#each emails as email}
                  <div class="marker-item">
                    <AppIcon name="mail" size={13} />
                    <span>{email}</span>
                    <button type="button" onclick={() => navigator.clipboard.writeText(email)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each urls as url}
                  <div class="marker-item">
                    <AppIcon name="globe" size={13} />
                    <a href={url} target="_blank" rel="noopener noreferrer">{url}</a>
                    <button type="button" onclick={() => navigator.clipboard.writeText(url)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each phones as phone}
                  <div class="marker-item">
                    <AppIcon name="phone" size={13} />
                    <span>{phone}</span>
                    <button type="button" onclick={() => navigator.clipboard.writeText(phone)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
                {#each colors as color}
                  <div class="marker-item color-marker">
                    <span class="color-swatch" style="background:{color}"></span>
                    <code>{color}</code>
                    <button type="button" onclick={() => navigator.clipboard.writeText(color)}>
                      <AppIcon name="copy" size={11} />
                    </button>
                  </div>
                {/each}
              </div>
            </div>
          {/if}
        </div>

      {:else if activeTab === "ocr"}
        <div class="ocr-section">
          {#if item.ocrStatus === "completed" && item.ocrText}
            <div class="ocr-status ocr-completed">
              <span class="ocr-dot"></span>
              {_t("detail.completed")}
            </div>
            <pre class="ocr-content">{item.ocrText}</pre>
          {:else if item.ocrStatus === "pending"}
            <div class="ocr-status ocr-pending">
              <span class="ocr-dot"></span>
              {_t("detail.pending")}
            </div>
          {:else}
            <div class="ocr-empty">
              <AppIcon name="image" size={32} strokeWidth={1.5} />
              <span>{_t("detail.noOcr")}</span>
            </div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

<style>
  .detail-backdrop {
    position: fixed;
    z-index: 51;
    inset: 0;
    background: rgba(0, 0, 0, 0.45);
    backdrop-filter: blur(3px);
  }

  .detail-panel {
    position: fixed;
    z-index: 52;
    top: 0;
    right: 0;
    bottom: 0;
    width: min(520px, 100vw);
    display: flex;
    flex-direction: column;
    border-left: 1px solid #363636;
    background: #1b1b1b;
    box-shadow: -8px 0 32px rgba(0, 0, 0, 0.5);
    animation: slide-in 220ms ease-out;
  }

  @keyframes slide-in {
    from { transform: translateX(100%); }
    to { transform: translateX(0); }
  }

  .detail-header {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 14px 16px;
    border-bottom: 1px solid #2a2a2a;
  }

  .back-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 1px solid #3a3a3a;
    border-radius: 7px;
    color: #999;
    background: #222;
    cursor: pointer;
    transition: color 100ms ease, background 100ms ease;
  }

  .back-btn:hover {
    color: #ddd;
    background: #2e2e2e;
  }

  .header-info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .header-kind {
    color: #777;
    font-size: 10px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }

  .header-title {
    overflow: hidden;
    color: #e4e4e4;
    font-size: 13px;
    font-weight: 540;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .detail-tabs {
    display: flex;
    gap: 2px;
    padding: 6px 12px;
    border-bottom: 1px solid #2a2a2a;
  }

  .detail-tabs button {
    padding: 7px 16px;
    border: 0;
    border-radius: 6px;
    color: #777;
    background: transparent;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    transition: color 100ms ease, background 100ms ease;
  }

  .detail-tabs button:hover {
    color: #bbb;
    background: #292929;
  }

  .detail-tabs button.active {
    color: #e4e4e4;
    background: #303030;
  }

  .detail-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px;
    scrollbar-width: thin;
    scrollbar-color: #555 transparent;
  }

  .preview-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .content-full {
    margin: 0;
    padding: 14px;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    color: #d7d7d7;
    background: #141414;
    font:
      12px/1.6 "Cascadia Code",
      Consolas,
      monospace;
    white-space: pre-wrap;
    word-break: break-word;
    overflow-x: auto;
  }

  .image-full-preview,
  .file-full-preview {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 180px;
    padding: 16px;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    color: #888;
    background: #141414;
  }

  .image-full-preview img {
    max-width: 100%;
    max-height: 400px;
    object-fit: contain;
    border-radius: 4px;
  }

  .image-placeholder {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .image-meta {
    color: #777;
    font-size: 11px;
  }

  .file-full-preview strong {
    color: #ddd;
    font-size: 13px;
  }

  .file-full-preview span {
    color: #777;
    font-size: 11px;
  }

  .detail-actions {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    padding-top: 8px;
  }

  .detail-actions button {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 8px 14px;
    border: 1px solid #3a3a3a;
    border-radius: 7px;
    color: #bbb;
    background: #222;
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition: background 100ms ease, border-color 100ms ease;
  }

  .detail-actions button:hover {
    background: #2e2e2e;
    border-color: #555;
    color: #e4e4e4;
  }

  .details-section {
    display: flex;
    flex-direction: column;
    gap: 16px;
  }

  .detail-list {
    display: grid;
    gap: 1px;
    margin: 0;
    padding: 0;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    overflow: hidden;
    background: #2e2e2e;
  }

  .detail-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: #141414;
    gap: 12px;
  }

  .detail-row dt {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: #888;
    font-size: 11px;
    font-weight: 500;
    flex-shrink: 0;
  }

  .detail-row dd {
    margin: 0;
    color: #ddd;
    font-size: 11px;
    text-align: right;
    word-break: break-all;
  }

  .detail-row dd code {
    padding: 2px 6px;
    border: 1px solid #333;
    border-radius: 4px;
    color: #b7b7b7;
    background: #1a1a1a;
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 10px;
  }

  .ocr-badge {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 10px;
  }

  .ocr-badge.ocr-completed {
    border: 1px solid #35513f;
    color: #9dc6aa;
    background: rgba(27, 45, 33, 0.6);
  }

  .ocr-badge.ocr-pending {
    border: 1px solid #4a4a35;
    color: #c6c69d;
    background: rgba(45, 45, 27, 0.6);
  }

  .special-section {
    padding: 12px;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    background: #141414;
  }

  .special-title {
    display: block;
    margin-bottom: 10px;
    color: #aaa;
    font-size: 11px;
    font-weight: 560;
  }

  .markers-list {
    display: grid;
    gap: 6px;
  }

  .marker-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid #303030;
    border-radius: 6px;
    color: #b2b2b2;
    font-size: 11px;
    background: #1a1a1a;
  }

  .marker-item span,
  .marker-item a {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .marker-item a {
    color: #66bde1;
    text-decoration: none;
  }

  .marker-item a:hover {
    text-decoration: underline;
  }

  .marker-item button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 1px solid transparent;
    border-radius: 4px;
    color: #666;
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 100ms ease;
  }

  .marker-item button:hover {
    color: #bbb;
  }

  .color-swatch {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid #444;
    flex-shrink: 0;
  }

  .color-marker code {
    padding: 2px 6px;
    border: 1px solid #333;
    border-radius: 4px;
    color: #b7b7b7;
    background: #1a1a1a;
    font: 10px "Cascadia Code", Consolas, monospace;
  }

  .ocr-section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .ocr-status {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    font-size: 12px;
  }

  .ocr-dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .ocr-completed {
    color: #9dc6aa;
  }

  .ocr-completed .ocr-dot {
    background: #51b96b;
    box-shadow: 0 0 6px rgba(81, 185, 107, 0.4);
  }

  .ocr-pending {
    color: #c6b06d;
  }

  .ocr-pending .ocr-dot {
    background: #d4b14c;
    box-shadow: 0 0 6px rgba(212, 177, 76, 0.4);
  }

  .ocr-content {
    margin: 0;
    padding: 14px;
    border: 1px solid #2e2e2e;
    border-radius: 8px;
    color: #d7d7d7;
    background: #141414;
    font: 12px/1.6 "Cascadia Code", Consolas, monospace;
    white-space: pre-wrap;
    word-break: break-word;
  }

  .ocr-empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 12px;
    min-height: 140px;
    color: #666;
    font-size: 12px;
  }

  @media (max-width: 520px) {
    .detail-panel {
      width: 100vw;
    }
  }

  .edit-area {
    position: relative;
    z-index: 4;
    padding: 4px;
  }

  .edit-area textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border: 1px solid #4aa8ff;
    border-radius: 7px;
    color: #e4e4e4;
    background: #141414;
    font: 12px/1.55 "Cascadia Code", Consolas, monospace;
    resize: vertical;
    outline: none;
  }

  .edit-actions {
    display: flex;
    gap: 6px;
    margin-top: 8px;
    justify-content: flex-end;
  }

  .edit-actions button {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    color: #a3a3a3;
    background: #252525;
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition: background 100ms ease, border-color 100ms ease;
  }

  .edit-actions button.edit-save {
    border-color: #e3e3e3;
    color: #1c1c1c;
    background: #e3e3e3;
  }

  .edit-actions button.edit-cancel:hover {
    color: #ccc;
    background: #2e2e2e;
  }
</style>
