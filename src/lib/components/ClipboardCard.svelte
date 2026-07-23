<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatRelativeTime } from "$lib/utils/time";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    item: ClipboardItem;
    index: number;
    now: number;
    selected: boolean;
    checked: boolean;
    showCheckbox: boolean;
    onselect: (id: string, event?: MouseEvent) => void;
    ontoggleSelect: (id: string) => void;
    ontoggleFavorite: (id: string) => void;
    ondelete: (id: string) => void;
    oncopy: (id: string) => void;
    ondetail: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void;
    oncanceledit: (id: string) => void;
    onplainpaste: (id: string) => void;
    onformatpaste: (id: string) => void;
  }

  let {
    item,
    index,
    now,
    selected,
    checked,
    showCheckbox,
    onselect,
    ontoggleSelect,
    ontoggleFavorite,
    ondelete,
    oncopy,
    ondetail,
    onedit,
    onsaveedit,
    oncanceledit,
    onplainpaste,
    onformatpaste,
  }: Props = $props();

  let editing = $state(false);
  let editContent = $state("");
  let contentActions = $state<{
    hasEmail: boolean;
    hasUrl: boolean;
    hasPhone: boolean;
    hasColor: boolean;
    emails: string[];
    urls: string[];
    phones: string[];
    colors: string[];
  } | null>(null);

  function detectInlineActions(): { hasEmail: boolean; hasUrl: boolean; hasPhone: boolean; hasColor: boolean; emails: string[]; urls: string[]; phones: string[]; colors: string[]; } {
    const text = [item.title, item.preview].filter(Boolean).join(" ");
    const emails = text.match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g) ?? [];
    const urls = text.match(/https?:\/\/[^\s)]+/g) ?? [];
    const phones = text.match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g) ?? [];
    const colors = text.match(/#(?:[0-9a-fA-F]{3}){1,2}\b/g) ?? [];

    return {
      hasEmail: emails.length > 0,
      hasUrl: urls.length > 0,
      hasPhone: phones.length > 0,
      hasColor: colors.length > 0,
      emails,
      urls,
      phones,
      colors,
    };
  }

  $effect(() => {
    if (!isTauriRuntime()) {
      contentActions = detectInlineActions();
      return;
    }
    void invoke<{
      hasEmail: boolean;
      hasUrl: boolean;
      hasPhone: boolean;
      hasColor: boolean;
      emails: string[];
      urls: string[];
      phones: string[];
      colors: string[];
    }>("detect_content_actions", { contentId: item.id })
      .then((actions) => {
        contentActions = actions;
      })
      .catch(() => {
        contentActions = detectInlineActions();
      });
  });

  function handleAction(event: MouseEvent, action: string, value: string) {
    event.stopPropagation();
    if (action === "email") {
      window.open(`mailto:${value}`, "_blank");
    } else if (action === "url") {
      window.open(value, "_blank");
    } else if (action === "phone") {
      window.open(`tel:${value}`, "_blank");
    } else if (action === "color") {
      void navigator.clipboard.writeText(value).catch(() => {});
    }
  }

  function handleDoubleClick(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    ondetail(item.id);
  }

  function handleDragStart(event: DragEvent) {
    if (!event.dataTransfer) return;

    if (item.kind === "text" || item.kind === "link") {
      event.dataTransfer.setData("text/plain", item.title);
      if (item.textContent) {
        event.dataTransfer.setData("text/html", item.textContent);
      }
      event.dataTransfer.effectAllowed = "copy";
    } else if (item.kind === "file" && item.resourcePath) {
      const fileUri = item.resourcePath.startsWith("file://")
        ? item.resourcePath
        : `file://${item.resourcePath.replace(/\\/g, "/")}`;
      event.dataTransfer.setData("text/uri-list", fileUri);
      event.dataTransfer.setData("text/plain", item.resourcePath);
      event.dataTransfer.effectAllowed = "copy";
    } else if (item.kind === "image") {
      event.dataTransfer.setData("text/plain", item.title);
      if (item.resourcePath) {
        const fileUri = item.resourcePath.startsWith("file://")
          ? item.resourcePath
          : `file://${item.resourcePath.replace(/\\/g, "/")}`;
        event.dataTransfer.setData("text/uri-list", fileUri);
      }
      event.dataTransfer.effectAllowed = "copy";
    }
  }

  function startEdit(event: MouseEvent) {
    event.stopPropagation();
    editContent = item.textContent || item.title;
    editing = true;
    onedit(item.id);
  }

  function saveEdit(event: Event) {
    event.stopPropagation();
    onsaveedit(item.id, editContent);
    editing = false;
  }

  function cancelEdit(event: Event) {
    event.stopPropagation();
    editing = false;
    oncanceledit(item.id);
  }
</script>

<article
  class:selected
  class="clip-card"
  draggable="true"
  ondragstart={handleDragStart}
>
  <button
    class="card-select"
    type="button"
    aria-label={_t("card.selectItem", { title: item.title })}
    aria-pressed={selected}
    onclick={(e) => onselect(item.id)}
    ondblclick={(e) => handleDoubleClick(e)}
  ></button>

  {#if showCheckbox}
    <label class="card-checkbox">
      <input
        type="checkbox"
        checked={checked}
        onchange={() => ontoggleSelect(item.id)}
        onclick={(e) => e.stopPropagation()}
      />
      <span class="check-mark"><AppIcon name="check" size={14} strokeWidth={2.5} /></span>
    </label>
  {/if}

  {#if !editing}
    <div class="content">
      {#if item.kind === "image"}
        {#if item.previewPath || item.resourcePath}
          <div class="image-preview">
            <img
              src={convertFileSrc(item.previewPath || item.resourcePath!)}
              alt={item.preview}
            />
          </div>
        {:else}
          <div class="image-preview image-placeholder">
            <AppIcon name="image" size={28} strokeWidth={1.5} />
          </div>
        {/if}
      {:else if item.kind === "file"}
        <div class="file-title">
          <span class="file-icon"><AppIcon name="file" size={15} /></span>
          <span>{item.fileName ?? item.title}</span>
        </div>
      {:else}
        <div class="text-preview">{item.title}</div>
        {#if item.preview}
          <div class="secondary-preview">{item.preview}</div>
        {/if}
      {/if}
    </div>

    <div class="meta-row">
      <span
        class:source-red={item.sourceTone === "red"}
        class:source-blue={item.sourceTone === "blue"}
        class:source-violet={item.sourceTone === "violet"}
        class="source-mark"
      >
        {#if item.sourceTone === "neutral"}
          <AppIcon name={item.kind === "file" ? "file" : "clipboard"} size={12} />
        {:else}
          <span class="source-dot"></span>
        {/if}
      </span>
      <span class="source-name">{item.sourceApp}</span>
      <span>{item.sizeLabel}</span>
      {#if item.detailLabel}<span>{item.detailLabel}</span>{/if}
      <span>{formatRelativeTime(item.createdAt, now)}</span>
      {#if item.kind === "file"}<span class="file-count">{item.preview}</span>{/if}
    </div>
  {:else}
    <div class="edit-area">
      <textarea
        bind:value={editContent}
        rows={Math.min(12, Math.max(3, editContent.split("\n").length))}
        placeholder={_t("edit.placeholder")}
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => {
          if (e.key === "Escape") cancelEdit(e);
        }}
      ></textarea>
      <div class="edit-actions">
        <button type="button" class="edit-save" onclick={saveEdit}>
          <AppIcon name="check" size={14} strokeWidth={2.5} /> {_t("edit.save")}
        </button>
        <button type="button" class="edit-cancel" onclick={cancelEdit}>
          <AppIcon name="x" size={14} strokeWidth={2.5} /> {_t("edit.cancel")}
        </button>
      </div>
    </div>
  {/if}

  {#if contentActions && (contentActions.hasEmail || contentActions.hasUrl || contentActions.hasPhone || contentActions.hasColor)}
    <div class="quick-actions" aria-label="Quick actions">
      {#if contentActions.hasUrl}
        <button type="button" title={_t("actions.openUrl")}
          onclick={(e) => handleAction(e, "url", contentActions!.urls[0])} class="qa-btn"
        ><AppIcon name="globe" size={13} /><span>{_t("actions.openUrl")}</span></button>
      {/if}
      {#if contentActions.hasEmail}
        <button type="button" title={_t("actions.sendEmail")}
          onclick={(e) => handleAction(e, "email", contentActions!.emails[0])} class="qa-btn"
        ><AppIcon name="mail" size={13} /><span>{_t("actions.sendEmail")}</span></button>
      {/if}
      {#if contentActions.hasPhone}
        <button type="button" title={_t("actions.callPhone")}
          onclick={(e) => handleAction(e, "phone", contentActions!.phones[0])} class="qa-btn"
        ><AppIcon name="phone" size={13} /><span>{_t("actions.callPhone")}</span></button>
      {/if}
      {#if contentActions.hasColor}
        <button type="button" title={_t("actions.copyColor")}
          onclick={(e) => handleAction(e, "color", contentActions!.colors[0])} class="qa-btn"
        ><AppIcon name="palette" size={13} /><span>{_t("actions.copyColor")}</span></button>
      {/if}
    </div>
  {/if}

  <div class="actions" aria-label={_t("card.itemActions")}>
    <button type="button" title={_t("card.viewDetail")} aria-label={_t("card.viewDetail")}
      onclick={(event) => { event.stopPropagation(); ondetail(item.id); }}
    ><AppIcon name="eye" size={16} /></button>
    <button type="button" title={_t("card.copy")} aria-label={_t("card.copy")}
      onclick={(event) => { event.stopPropagation(); oncopy(item.id); }}
    ><AppIcon name="copy" size={16} /></button>
    {#if item.kind === "image" || item.kind === "file"}
      <button type="button" title={_t("card.export")} aria-label={_t("card.export")}
        ><AppIcon name="download" size={16} /></button
      >
    {/if}
    {#if item.kind === "text" || item.kind === "link"}
      <button type="button" title={_t("card.edit")} aria-label={_t("card.edit")}
        onclick={startEdit}
      ><AppIcon name="edit" size={16} /></button>
    {/if}
    {#if item.kind === "text"}
      <button type="button" title={_t("card.pastePlain")} aria-label={_t("card.pastePlain")}
        onclick={(event) => { event.stopPropagation(); onplainpaste(item.id); }}
      ><AppIcon name="type" size={16} /></button>
    {/if}
    <button type="button" title={_t("card.pasteFormat")} aria-label={_t("card.pasteFormat")}
      onclick={(event) => { event.stopPropagation(); onformatpaste(item.id); }}
    ><AppIcon name="copy-plus" size={16} /></button>
    <button
      type="button"
      class:active={item.favorite}
      title={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
      aria-label={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
      onclick={(event) => {
        event.stopPropagation();
        ontoggleFavorite(item.id);
      }}><AppIcon name="star" size={16} filled={item.favorite} /></button
    >
    {#if !item.favorite}
      <button
        type="button"
        title={_t("card.delete")}
        aria-label={_t("card.delete")}
        onclick={(event) => {
          event.stopPropagation();
          ondelete(item.id);
        }}><AppIcon name="trash" size={16} /></button
      >
    {/if}
  </div>

  <span class="shortcut">⌘{index + 1}</span>
</article>

<style>
  .clip-card {
    position: relative;
    padding: 13px 14px 12px;
    border: 1px solid transparent;
    border-radius: 10px;
    color: #ececec;
    background: transparent;
    cursor: default;
    transition:
      background 120ms ease,
      border-color 120ms ease;
  }

  .card-select {
    position: absolute;
    z-index: 0;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    border-radius: inherit;
    background: transparent;
    cursor: default;
  }

  .card-checkbox {
    position: absolute;
    z-index: 3;
    left: 10px;
    top: 13px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .clip-card:hover .card-checkbox,
  .clip-card .card-checkbox:has(input:checked) {
    opacity: 1;
  }

  .card-checkbox input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
  }

  .check-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    border: 1.5px solid #5a5a5a;
    border-radius: 5px;
    color: transparent;
    background: transparent;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .card-checkbox input:checked + .check-mark {
    border-color: #4aa8ff;
    background: #4aa8ff;
    color: #fff;
  }

  .clip-card:hover,
  .clip-card.selected {
    border-color: rgba(255, 255, 255, 0.035);
    background: #242424;
  }

  .content {
    position: relative;
    z-index: 1;
    min-width: 0;
    padding-right: 76px;
    padding-left: 0;
    pointer-events: none;
  }

  .clip-card:has(.card-checkbox) .content {
    padding-left: 28px;
  }

  .text-preview {
    overflow: hidden;
    font-size: 13px;
    line-height: 1.55;
    white-space: pre-line;
    text-overflow: ellipsis;
  }

  .secondary-preview {
    margin-top: 4px;
    overflow: hidden;
    color: #8e8e8e;
    font-size: 11.5px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file-title {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
    font-size: 13px;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file-icon {
    display: inline-flex;
    color: #d7d7d7;
  }

  .image-preview {
    width: min(100%, 380px);
    overflow: hidden;
    border: 1px solid #303237;
    border-radius: 6px;
    background: #17191d;
    box-shadow: inset 0 0 40px rgba(0, 0, 0, 0.3);
  }

  .image-preview img {
    display: block;
    max-height: 120px;
    width: 100%;
    object-fit: contain;
    border-radius: 6px;
  }

  .image-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 82px;
    color: #555;
  }

  .meta-row {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
    margin-top: 10px;
    padding-right: 82px;
    color: #8c8c8c;
    font-size: 11.5px;
    white-space: nowrap;
    pointer-events: none;
  }

  .source-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    color: #d7c47b;
  }

  .source-dot {
    width: 10px;
    height: 10px;
    border-radius: 3px 6px 3px 6px;
    background: currentColor;
    transform: rotate(-12deg);
  }
  .source-red {
    color: #ff4655;
  }
  .source-blue {
    color: #66bde1;
  }
  .source-violet {
    color: #746dff;
  }
  .source-name {
    color: #aaaaaa;
  }
  .file-count {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .quick-actions {
    position: relative;
    z-index: 2;
    display: flex;
    gap: 4px;
    margin-top: 8px;
    flex-wrap: wrap;
  }

  .qa-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 8px;
    border: 1px solid #3a3a3a;
    border-radius: 14px;
    color: #b2b2b2;
    background: #1e1e1e;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease;
  }

  .qa-btn:hover {
    border-color: #5a5a5a;
    background: #2c2c2c;
    color: #e8e8e8;
  }

  .actions {
    position: absolute;
    z-index: 2;
    right: 34px;
    bottom: 9px;
    display: flex;
    gap: 2px;
    opacity: 0;
    transform: translateY(2px);
    transition:
      opacity 120ms ease,
      transform 120ms ease;
  }

  .clip-card:hover .actions,
  .clip-card.selected .actions,
  .actions:focus-within {
    opacity: 1;
    transform: translateY(0);
  }

  .actions button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 27px;
    height: 27px;
    padding: 0;
    border: 0;
    border-radius: 6px;
    color: #777777;
    background: transparent;
    cursor: pointer;
  }

  .actions button:hover,
  .actions button.active {
    color: #f0cb42;
    background: #303030;
  }

  .shortcut {
    position: absolute;
    z-index: 1;
    right: 11px;
    bottom: 15px;
    color: #747474;
    font-size: 11.5px;
    pointer-events: none;
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

  .edit-save,
  .edit-cancel {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 5px 12px;
    border: 1px solid #3a3a3a;
    border-radius: 5px;
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition: background 100ms ease, border-color 100ms ease;
  }

  .edit-save {
    border-color: #4aa8ff;
    color: #4aa8ff;
    background: rgba(74, 168, 255, 0.1);
  }

  .edit-save:hover {
    background: rgba(74, 168, 255, 0.2);
  }

  .edit-cancel {
    color: #999;
    background: #222;
  }

  .edit-cancel:hover {
    color: #ccc;
    background: #2e2e2e;
  }

  @media (max-width: 620px) {
    .content {
      padding-right: 40px;
    }
    .meta-row {
      padding-right: 28px;
    }
    .actions {
      display: none;
    }
    .quick-actions {
      display: none;
    }
  }
</style>
