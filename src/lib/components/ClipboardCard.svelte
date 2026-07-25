<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatRelativeTime } from "$lib/utils/time";
  import { isTauriRuntime } from "$lib/services/runtime";
  import { detectContentActions, type QuickAction } from "$lib/services/clipboard";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { iconsDir } from "$lib/services/paths";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  function assetUrl(filePath: string | null | undefined): string | undefined {
    if (!filePath) return undefined;
    if (!isTauriRuntime()) return undefined;
    try {
      const normalized = filePath.replace(/\\/g, "/");
      return convertFileSrc(normalized);
    } catch (e) {
      return undefined;
    }
  }

  function appIconUrl(iconFileName: string | null | undefined): string | undefined {
    if (!iconFileName || !isTauriRuntime()) return undefined;
    let dir = "";
    const unsub = iconsDir.subscribe((v) => {
      dir = v;
    });
    unsub();
    if (!dir) return undefined;
    const fullPath = `${dir}/${iconFileName}`.replace(/\\/g, "/");
    return convertFileSrc(fullPath);
  }

  interface Props {
    item: ClipboardItem;
    index: number;
    now: number;
    selected: boolean;
    checked: boolean;
    showCheckbox: boolean;
    compact?: boolean;
    compactPaddingTop?: number;
    compactPaddingBottom?: number;
    compactCardGap?: number;
    compactCardBorderRadius?: number;
    compactCardHeight?: number;
    maxTextLines?: number;
    showSecondaryText?: boolean;
    hideActions?: boolean;
    onselect: (id: string, event?: MouseEvent) => void;
    ontoggleSelect: (id: string) => void;
    ontoggleFavorite: (id: string) => void;
    ondelete: (id: string) => void;
    oncopy: (id: string) => void;
    ondetail: (id: string) => void;
    onedit: (id: string) => void;
    onsaveedit: (id: string, content: string) => void | Promise<boolean>;
    oncanceledit: (id: string) => void;
    onplainpaste: (id: string) => void;
    onsaveasnew: (id: string, title: string, content: string) => void;
    onrestore?: (id: string) => void;
    onheightchange?: (id: string, height: number) => void;
    heightMeasurementKey?: string;
  }

  const cardActionIds = [
    "copy",
    "plainpaste",
    "detail",
    "edit",
    "favorite",
    "delete",
    "restore",
  ] as const;
  type CardActionId = (typeof cardActionIds)[number];

  let {
    item,
    index,
    now,
    selected,
    checked,
    showCheckbox,
    compact = false,
    compactPaddingTop = 6,
    compactPaddingBottom = 4,
    compactCardGap = 5,
    compactCardBorderRadius = 10,
    compactCardHeight = 0,
    maxTextLines = 3,
    showSecondaryText = true,
    hideActions = false,
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
    onsaveasnew,
    onrestore,
    onheightchange,
    heightMeasurementKey,
  }: Props = $props();

  let contextMenu = $state<{ x: number; y: number; items: ContextMenuItem[] } | null>(null);
  let cardElement = $state<HTMLDivElement | null>(null);

  let editing = $state(false);
  let editContent = $state("");
  let editTitle = $state("");
  let editTextarea = $state<HTMLTextAreaElement | null>(null);

  $effect(() => {
    const element = cardElement;
    const reportHeight = onheightchange;
    void heightMeasurementKey;
    if (!element || !reportHeight || typeof ResizeObserver === "undefined") return;

    let lastHeight = -1;
    const report = () => {
      const height = Math.ceil(
        element.getBoundingClientRect().height + (compact ? compactCardGap : 0),
      );
      if (height === lastHeight) return;
      lastHeight = height;
      reportHeight(item.id, height);
    };
    const observer = new ResizeObserver(report);
    observer.observe(element);
    report();
    return () => observer.disconnect();
  });

  $effect(() => {
    if (!editing || !editTextarea) return;
    const textarea = editTextarea;
    queueMicrotask(() => textarea.focus());
  });

  const contentChanged = $derived(
    editContent !== (item.textContent || item.title) || editTitle !== item.title,
  );
  let contentActions = $state<QuickAction[]>([]);
  let contentActionRequest = 0;

  function detectInlineActions(text: string): QuickAction[] {
    const emails = text.match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g) ?? [];
    const urls = text.match(/https?:\/\/[^\s)]+/g) ?? [];
    const phones =
      text.match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g) ?? [];
    const colors = text.match(/#(?:[0-9a-fA-F]{3}){1,2}\b/g) ?? [];

    return [
      ...emails.map((value) => ({
        label: `Send email to ${value}`,
        actionType: "open" as const,
        payload: `mailto:${value}`,
      })),
      ...phones.map((value) => ({
        label: `Call ${value}`,
        actionType: "open" as const,
        payload: `tel:${value.replace(/[^+\d]/g, "")}`,
      })),
      ...urls.map((value) => ({
        label: `Open ${value}`,
        actionType: "open" as const,
        payload: value,
      })),
      ...colors.map((value) => ({
        label: `Copy color ${value}`,
        actionType: "copy" as const,
        payload: value,
      })),
    ];
  }

  $effect(() => {
    const text = item.textContent || [item.title, item.preview].filter(Boolean).join("\n");
    const request = ++contentActionRequest;
    if (!isTauriRuntime()) {
      contentActions = detectInlineActions(text);
      return;
    }
    void detectContentActions(text)
      .then((actions) => {
        if (request === contentActionRequest) {
          contentActions = actions ?? detectInlineActions(text);
        }
      })
      .catch(() => {
        if (request === contentActionRequest) {
          contentActions = detectInlineActions(text);
        }
      });
  });

  function quickActionKind(action: QuickAction): "url" | "email" | "phone" | "color" | "copy" {
    if (action.payload.startsWith("mailto:")) return "email";
    if (action.payload.startsWith("tel:")) return "phone";
    if (/^https?:\/\//i.test(action.payload)) return "url";
    if (/^(?:#[0-9a-f]{3,8}|rgba?\(|hsla?\()/i.test(action.payload)) return "color";
    return "copy";
  }

  async function handleAction(event: MouseEvent, action: QuickAction) {
    event.stopPropagation();
    if (action.actionType === "open") {
      try {
        await invoke("open_external_url", { url: action.payload });
      } catch {
        window.open(action.payload, "_blank");
      }
    } else {
      void navigator.clipboard.writeText(action.payload).catch(() => {});
    }
  }

  function handleDoubleClick(event: MouseEvent) {
    event.preventDefault();
    runCardAction("detail", event);
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

  function beginEdit() {
    editContent = item.textContent || item.title;
    editTitle = item.title;
    editing = true;
    onedit(item.id);
  }

  function runCardAction(action: CardActionId, event?: Event) {
    event?.stopPropagation();

    switch (action) {
      case "copy":
        oncopy(item.id);
        return;
      case "plainpaste":
        onplainpaste(item.id);
        return;
      case "detail":
        ondetail(item.id);
        return;
      case "edit":
        beginEdit();
        return;
      case "favorite":
        ontoggleFavorite(item.id);
        return;
      case "delete":
        ondelete(item.id);
        return;
      case "restore":
        onrestore?.(item.id);
        return;
    }
  }

  async function saveEdit(event: Event) {
    event.stopPropagation();
    const saved = await onsaveedit(item.id, editContent);
    if (saved !== false) editing = false;
  }

  function cancelEdit(event: Event) {
    event.stopPropagation();
    editing = false;
    oncanceledit(item.id);
  }

  function saveAsNew(event: Event) {
    event.stopPropagation();
    onsaveasnew(item.id, editTitle, editContent);
    editing = false;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (editing && event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      editing = false;
      oncanceledit(item.id);
    }
  }

  function handleContextMenu(event: MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    const canEdit =
      (item.kind === "text" ||
        item.kind === "link" ||
        item.kind === "image" ||
        item.kind === "file") &&
      (!item.fileMeta || item.fileMeta.length <= 1);
    const items: ContextMenuItem[] = [
      { id: "copy", label: _t("card.copy"), icon: "copy" },
      ...(item.kind === "text"
        ? [{ id: "plainpaste", label: _t("card.pastePlain"), icon: "type" as IconName }]
        : []),
      { id: "detail", label: _t("card.viewDetail"), icon: "eye" },
      ...(canEdit
        ? [
            {
              id: "edit",
              label:
                item.kind === "image" || item.kind === "file"
                  ? _t("edit.editFileName")
                  : _t("card.edit"),
              icon: "edit" as IconName,
            },
          ]
        : []),
      {
        id: "favorite",
        label: item.favorite ? _t("card.unfavorite") : _t("card.favorite"),
        icon: "star",
      },
      ...(!item.favorite
        ? [
            {
              id: "delete",
              label: _t("card.delete"),
              icon: "trash" as IconName,
              destructive: true,
            },
          ]
        : []),
    ];
    contextMenu = { x: event.clientX, y: event.clientY, items };
  }

  function handleContextAction(id: string) {
    if (cardActionIds.includes(id as CardActionId)) {
      runCardAction(id as CardActionId);
    }
  }
</script>

<div
  bind:this={cardElement}
  role="button"
  aria-pressed={selected}
  aria-label={item.title}
  class:selected
  class:compact
  class:editing
  class="clip-card"
  style:--cpt={compact ? `${compactPaddingTop}px` : undefined}
  style:--cpb={compact ? `${compactPaddingBottom}px` : undefined}
  style:--cg={compact ? `${compactCardGap}px` : undefined}
  style:--cbr={compact ? `${compactCardBorderRadius}px` : undefined}
  style:--max-text-lines={`${showSecondaryText ? maxTextLines : 1}`}
  style:min-height={editing
    ? "auto"
    : compact && compactCardHeight
      ? `${compactCardHeight}px`
      : undefined}
  tabindex="-1"
  data-id={item.id}
  draggable="true"
  ondragstart={handleDragStart}
  oncontextmenu={handleContextMenu}
  onfocus={() => onselect(item.id)}
  onkeydown={handleKeydown}
>
  <button
    class="card-select"
    type="button"
    aria-label={_t("card.selectItem", { title: item.title })}
    aria-pressed={selected}
    onclick={(e) => onselect(item.id, e)}
    ondblclick={(e) => handleDoubleClick(e)}
  ></button>

  {#if showCheckbox}
    <label class="card-checkbox">
      <input
        type="checkbox"
        {checked}
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
            {#if assetUrl(item.previewPath || item.resourcePath)}
              <img src={assetUrl(item.previewPath || item.resourcePath)} alt={item.preview || ""} />
            {:else}
              <AppIcon name="image" size={28} strokeWidth={1.5} />
            {/if}
          </div>
        {:else}
          <div class="image-preview image-placeholder">
            <AppIcon name="image" size={28} strokeWidth={1.5} />
          </div>
        {/if}
      {:else if item.kind === "file"}
        <div class="file-title">
          <span class="file-icon"><AppIcon name="file" size={15} /></span>
          {#if item.fileMeta && item.fileMeta.length > 1}
            <span
              >{item.fileMeta[0].name}{item.fileMeta.length > 2 ? `, ${item.fileMeta[1].name}` : ""} 等
              {item.fileMeta.length} 个文件</span
            >
          {:else}
            <span>{item.fileName ?? item.title}</span>
          {/if}
        </div>
      {:else}
        <div class="text-preview" class:custom-title={item.customTitle}>
          {item.customTitle ? item.title : item.textContent || item.title}
        </div>
        {#if item.customTitle && showSecondaryText && (item.textContent || item.preview)}
          <div class="content-preview">{item.textContent || item.preview}</div>
        {/if}
      {/if}
    </div>

    <div class="meta-row">
      <span class="source-mark">
        {#if item.iconPath}
          <img class="source-icon" src={appIconUrl(item.iconPath)} alt={item.sourceApp} />
        {:else}
          <span
            class="source-dot"
            class:source-red={item.sourceTone === "red"}
            class:source-blue={item.sourceTone === "blue"}
            class:source-violet={item.sourceTone === "violet"}
          ></span>
        {/if}
      </span>
      <span class="source-name">{item.sourceApp}</span>
      <span>{item.sizeLabel}</span>
      {#if item.detailLabel}<span>{item.detailLabel}</span>{/if}
      <span>{formatRelativeTime(item.createdAt, now)}</span>
      {#if item.kind === "file"}<span class="file-count">{item.preview}</span>{/if}
      <div class="actions" aria-label={_t("card.itemActions")} class:actions-hidden={hideActions}>
        {#each contentActions as action (`${action.actionType}:${action.payload}`)}
          <button
            type="button"
            title={action.label}
            aria-label={action.label}
            onclick={(event) => handleAction(event, action)}
          >
            {#if quickActionKind(action) === "url"}
              <AppIcon name="globe" size={16} />
            {:else if quickActionKind(action) === "email"}
              <AppIcon name="mail" size={16} />
            {:else if quickActionKind(action) === "phone"}
              <AppIcon name="phone" size={16} />
            {:else if quickActionKind(action) === "color"}
              <AppIcon name="palette" size={16} />
            {:else}
              <AppIcon name="copy" size={16} />
            {/if}
          </button>
        {/each}
        <button
          type="button"
          title={_t("card.viewDetail")}
          aria-label={_t("card.viewDetail")}
          onclick={(event) => runCardAction("detail", event)}
          ><AppIcon name="eye" size={16} /></button
        >
        <button
          type="button"
          title={_t("card.copy")}
          aria-label={_t("card.copy")}
          onclick={(event) => runCardAction("copy", event)}
          ><AppIcon name="copy" size={16} /></button
        >
        {#if item.kind === "image" || item.kind === "file"}
          <button
            type="button"
            title={_t("card.saveAs")}
            aria-label={_t("card.saveAs")}
            onclick={async (event) => {
              event.stopPropagation();
              if (item.resourcePath && isTauriRuntime()) {
                try {
                  const { save } = await import("@tauri-apps/plugin-dialog");
                  const defaultName = item.fileName || item.title.split(/[\\/]/).pop() || "file";
                  const ext = defaultName.includes(".") ? defaultName.split(".").pop() : "";
                  const filters = ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : [];
                  const filePath = await save({ defaultPath: defaultName, filters });
                  if (filePath) {
                    await invoke("copy_file_to", { src: item.resourcePath, dst: filePath });
                  }
                } catch {
                  invoke("open_external_url", { url: item.resourcePath }).catch(() => {});
                }
              }
            }}><AppIcon name="download" size={16} /></button
          >
        {/if}
        {#if (item.kind === "text" || item.kind === "link" || item.kind === "image" || item.kind === "file") && (!item.fileMeta || item.fileMeta.length <= 1)}
          <button
            type="button"
            title={item.kind === "image" || item.kind === "file"
              ? _t("edit.editFileName")
              : _t("card.edit")}
            aria-label={item.kind === "image" || item.kind === "file"
              ? _t("edit.editFileName")
              : _t("card.edit")}
            onclick={(event) => runCardAction("edit", event)}
            ><AppIcon name="edit" size={16} /></button
          >
        {/if}
        {#if item.kind === "text"}
          <button
            type="button"
            title={_t("card.pastePlain")}
            aria-label={_t("card.pastePlain")}
            onclick={(event) => runCardAction("plainpaste", event)}
            ><AppIcon name="type" size={16} /></button
          >
        {/if}
        <button
          type="button"
          class:active={item.favorite}
          title={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
          aria-label={item.favorite ? _t("card.unfavorite") : _t("card.favorite")}
          onclick={(event) => runCardAction("favorite", event)}
          ><AppIcon name="star" size={16} filled={item.favorite} /></button
        >
        {#if item.deleted && onrestore}
          <button
            type="button"
            title="恢复"
            aria-label="恢复"
            onclick={(event) => runCardAction("restore", event)}
            ><AppIcon name="edit" size={16} /></button
          >
        {/if}
        {#if !item.favorite}
          <button
            type="button"
            title={_t("card.delete")}
            aria-label={_t("card.delete")}
            onclick={(event) => runCardAction("delete", event)}
            ><AppIcon name="trash" size={16} /></button
          >
        {/if}
      </div>
      <span class="shortcut">⌘{index + 1}</span>
    </div>
  {:else}
    <div class="edit-area">
      {#if item.customTitle}
        <input class="edit-title-input" bind:value={editTitle} placeholder="标题" />
      {/if}
      <textarea
        bind:value={editContent}
        bind:this={editTextarea}
        rows={Math.min(12, Math.max(3, editContent.split("\n").length))}
        placeholder={_t("edit.placeholder")}
        onclick={(e) => e.stopPropagation()}
        onkeydown={(e) => {
          if (e.key === "Escape") {
            e.preventDefault();
            e.stopPropagation();
            editing = false;
            oncanceledit(item.id);
          }
        }}></textarea>
      <div class="edit-actions">
        <button type="button" class="edit-save" onclick={saveEdit}>
          <AppIcon name="check" size={14} strokeWidth={2.5} />
          {_t("edit.save")}
        </button>
        <button
          type="button"
          class="edit-save-as-new"
          disabled={!contentChanged}
          onclick={saveAsNew}
        >
          <AppIcon name="copy" size={14} strokeWidth={2.5} />
          {_t("edit.saveAsNew")}
        </button>
        <button type="button" class="edit-cancel" onclick={cancelEdit}>
          <AppIcon name="x" size={14} strokeWidth={2.5} />
          {_t("edit.cancel")}
        </button>
      </div>
    </div>
  {/if}
</div>

{#if contextMenu}
  <ContextMenu
    x={contextMenu.x}
    y={contextMenu.y}
    items={contextMenu.items}
    onclose={() => {
      contextMenu = null;
    }}
    onaction={handleContextAction}
  />
{/if}

<style>
  .clip-card {
    position: relative;
    padding: 13px 14px 12px;
    border: 1px solid transparent;
    border-radius: 10px;
    color: #ececec;
    background: transparent;
    cursor: default;
    overflow: hidden;
    outline: none;
    transition: background 120ms ease;
  }

  .clip-card.compact {
    padding: var(--cpt, 6px) 14px var(--cpb, 4px);
    border-radius: var(--cbr, 7px);
    margin-bottom: var(--cg, 5px);
    box-sizing: border-box;
    overflow: hidden;
  }

  .clip-card.editing {
    overflow: visible;
    z-index: 2;
  }

  .clip-card.compact .meta-row {
    margin-top: 0;
    gap: 5px;
  }

  .clip-card.compact .meta-row span {
    font-size: 9.5px;
  }

  .clip-card.compact .content {
    font-size: 12px;
  }

  .clip-card.compact .text-preview {
    font-size: var(--font-size-cardTitle, 13px);
    line-height: 1.4;
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
  .clip-card.selected .card-checkbox,
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
  .clip-card.selected,
  .clip-card:focus-within {
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
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: var(--max-text-lines, 3);
    line-clamp: var(--max-text-lines, 3);
    font-size: var(--font-size-cardTitle, 13px);
    line-height: 1.55;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    text-overflow: ellipsis;
  }

  .text-preview.custom-title {
    display: block;
    white-space: nowrap;
  }

  .content-preview {
    margin-top: 4px;
    overflow: hidden;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: var(--max-text-lines, 3);
    line-clamp: var(--max-text-lines, 3);
    color: #8e8e8e;
    font-size: var(--font-size-cardPreview, 11px);
    line-height: 1.45;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    text-overflow: ellipsis;
  }

  .file-title {
    display: flex;
    align-items: center;
    gap: 8px;
    overflow: hidden;
    font-size: var(--font-size-base, 13px);
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .file-icon {
    display: inline-flex;
    color: #d7d7d7;
  }

  .image-preview {
    width: min(100%, 380px);
    height: 90px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid #303237;
    border-radius: 6px;
    background: #17191d;
    box-shadow: inset 0 0 40px rgba(0, 0, 0, 0.3);
  }

  .image-preview img {
    display: block;
    width: 100%;
    height: 100%;
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
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 8px;
    min-width: 0;
    margin-top: 10px;
    color: #8c8c8c;
    font-size: var(--font-size-secondary, 11.5px);
    white-space: nowrap;
    pointer-events: none;
  }

  .source-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    color: #d7c47b;
  }

  .source-icon {
    width: 16px;
    height: 16px;
    object-fit: contain;
    border-radius: 2px;
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

  .actions {
    display: flex;
    gap: 2px;
    margin-left: auto;
    opacity: 0;
    pointer-events: auto;
    transition: opacity 120ms ease;
  }

  .actions-hidden {
    opacity: 0 !important;
    pointer-events: none !important;
  }

  .clip-card:hover .actions,
  .clip-card.selected .actions,
  .clip-card:hover .shortcut,
  .clip-card.selected .shortcut {
    opacity: 1;
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
    margin-left: 8px;
    color: #747474;
    font-size: 11.5px;
    pointer-events: none;
    flex-shrink: 0;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .edit-area {
    position: relative;
    z-index: 1;
    padding: 4px;
  }

  .edit-title-input {
    width: 100%;
    padding: 6px 8px;
    margin-bottom: 6px;
    border: 1px solid #3a3a3a;
    border-radius: 6px;
    color: #d8d8d8;
    background: #1a1a1a;
    font: inherit;
    font-size: 12px;
    outline: none;
    box-sizing: border-box;
  }

  .edit-title-input:focus {
    border-color: #5a5a5a;
  }

  .edit-area textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border: 1px solid #4aa8ff;
    border-radius: 7px;
    color: #e4e4e4;
    background: #141414;
    font:
      12px/1.55 "Cascadia Code",
      Consolas,
      monospace;
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
  .edit-save-as-new,
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
    transition:
      background 100ms ease,
      border-color 100ms ease;
  }

  .edit-save {
    color: #d8d8d8;
    background: #2a2a2a;
    border-color: #4a4a4a;
  }

  .edit-save:hover {
    color: #fff;
    background: #383838;
    border-color: #5a5a5a;
  }

  .edit-save-as-new {
    color: #999;
    background: #252525;
    border-color: #3a3a3a;
  }

  .edit-save-as-new:hover {
    color: #bbb;
    background: #303030;
    border-color: #4a4a4a;
  }

  .edit-save-as-new:disabled {
    opacity: 0.35;
    cursor: default;
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
    .actions {
      display: none;
    }
  }
</style>
