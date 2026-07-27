<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { IconName } from "$lib/components/AppIcon.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import type { ContextMenuItem } from "$lib/components/ContextMenu.svelte";
  import type { ClipboardItem } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { formatRelativeTime } from "$lib/utils/time";
  import { isTauriRuntime } from "$lib/services/runtime";
  import {
    detectContentActions,
    type QuickAction,
    writeClipboardText,
    getDisplayTitle,
    getDisplayRemainingLines,
  } from "$lib/services/clipboard";
  import { trimTrailingBlankLines } from "$lib/utils/virtual-scroll";
  import { invoke, convertFileSrc } from "@tauri-apps/api/core";
  import { iconsDir } from "$lib/services/paths";
  import { tick } from "svelte";

  let iconsBase = $derived($iconsDir);

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
    if (!iconFileName || !isTauriRuntime() || !iconsBase) return undefined;
    const fullPath = `${iconsBase}/${iconFileName}`.replace(/\\/g, "/");
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
    alwaysShowActions?: boolean;
    quickCopyBadgeAlwaysVisible?: boolean;
    hideMetaRow?: boolean;
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
    onimagefullscreen?: (id: string) => void;
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
    alwaysShowActions = false,
    quickCopyBadgeAlwaysVisible = true,
    hideMetaRow = false,
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
    onimagefullscreen,
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
  const primaryPreviewText = $derived(
    trimTrailingBlankLines(item.textContent) || trimTrailingBlankLines(item.title),
  );
  const primaryFirstLine = $derived(getDisplayTitle(primaryPreviewText));
  const primaryRestLines = $derived(getDisplayRemainingLines(primaryPreviewText));
  const secondaryPreviewText = $derived(
    trimTrailingBlankLines(item.textContent) || trimTrailingBlankLines(item.preview),
  );
  let contentActions = $state<QuickAction[]>([]);
  let contentActionRequest = 0;
  let dateDialog = $state<HTMLDialogElement | null>(null);
  let dateView = $state<{ isoDate: string; formattedDate: string; label: string } | null>(null);

  function parseIsoDate(value: string): Date | null {
    const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
    if (!match) return null;

    const year = Number(match[1]);
    const month = Number(match[2]);
    const day = Number(match[3]);
    if (year < 1 || month < 1 || month > 12 || day < 1) return null;

    const date = new Date(0);
    date.setUTCHours(12, 0, 0, 0);
    date.setUTCFullYear(year, month - 1, day);
    if (
      date.getUTCFullYear() !== year ||
      date.getUTCMonth() !== month - 1 ||
      date.getUTCDate() !== day
    ) {
      return null;
    }
    return date;
  }

  function normalizeInlineDate(year: number, month: number, day: number): string | null {
    const isoDate = `${String(year).padStart(4, "0")}-${String(month).padStart(2, "0")}-${String(day).padStart(2, "0")}`;
    return parseIsoDate(isoDate) ? isoDate : null;
  }

  function detectInlineDateValues(text: string): string[] {
    const values: string[] = [];
    const add = (value: string | null) => {
      if (value && !values.includes(value)) values.push(value);
    };

    for (const pattern of [
      /\b(\d{4})[-/](\d{1,2})[-/](\d{1,2})\b/g,
      /\b(\d{4})年(\d{1,2})月(\d{1,2})日/g,
    ]) {
      for (const match of text.matchAll(pattern)) {
        add(normalizeInlineDate(Number(match[1]), Number(match[2]), Number(match[3])));
      }
    }

    for (const match of text.matchAll(/\b(\d{1,2})[-/](\d{1,2})[-/](\d{4})\b/g)) {
      const first = Number(match[1]);
      const second = Number(match[2]);
      const year = Number(match[3]);
      const dayFirst = normalizeInlineDate(year, second, first);
      const monthFirst = normalizeInlineDate(year, first, second);
      if (dayFirst && monthFirst && dayFirst !== monthFirst) continue;
      add(dayFirst ?? monthFirst);
    }

    return values;
  }

  function detectInlineActions(text: string): QuickAction[] {
    const emails = text.match(/[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}/g) ?? [];
    const urls = text.match(/https?:\/\/[^\s)]+/g) ?? [];
    const phones =
      text.match(/(?:\+?\d{1,3}[-.\s]?)?\(?\d{2,4}\)?[-.\s]?\d{3,4}[-.\s]?\d{4,}/g) ?? [];
    const colors = text.match(/#(?:[0-9a-fA-F]{3}){1,2}\b/g) ?? [];
    const dates = detectInlineDateValues(text);

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
      ...dates.map((value) => ({
        label: `View date ${value}`,
        actionType: "viewDate" as const,
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

  function quickActionKind(
    action: QuickAction,
  ): "url" | "email" | "phone" | "date" | "color" | "copy" {
    if (action.actionType === "viewDate") return "date";
    if (action.payload.startsWith("mailto:")) return "email";
    if (action.payload.startsWith("tel:")) return "phone";
    if (/^https?:\/\//i.test(action.payload)) return "url";
    if (/^(?:#[0-9a-f]{3,8}|rgba?\(|hsla?\()/i.test(action.payload)) return "color";
    return "copy";
  }

  async function showDateDialog(action: QuickAction) {
    const date = parseIsoDate(action.payload);
    if (!date) {
      console.warn("Ignored invalid date action payload", action.payload);
      return;
    }

    dateView = {
      isoDate: action.payload,
      formattedDate: new Intl.DateTimeFormat(undefined, {
        dateStyle: "full",
        timeZone: "UTC",
      }).format(date),
      label: action.label,
    };
    await tick();
    if (!dateDialog || dateDialog.open) return;
    try {
      dateDialog.showModal();
    } catch {
      dateDialog.setAttribute("open", "");
    }
  }

  function closeDateDialog() {
    if (dateDialog?.open) {
      try {
        dateDialog.close();
      } catch {
        dateDialog.removeAttribute("open");
      }
    }
    dateView = null;
  }

  function handleDateDialogKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    closeDateDialog();
  }

  function handleDateDialogClick(event: MouseEvent) {
    if (event.target === event.currentTarget) closeDateDialog();
  }

  async function handleAction(event: MouseEvent, action: QuickAction) {
    event.stopPropagation();
    switch (action.actionType) {
      case "open":
        try {
          await invoke("open_external_url", { url: action.payload });
        } catch {
          window.open(action.payload, "_blank");
        }
        return;
      case "copy":
        void writeClipboardText(action.payload).catch(() => {});
        return;
      case "viewDate":
        await showDateDialog(action);
        return;
      default: {
        const unsupportedAction: never = action.actionType;
        console.warn("Ignored unsupported quick action", unsupportedAction);
      }
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
  role="option"
  aria-selected={selected}
  aria-label={item.title}
  class:selected
  class:checked
  class:compact
  class:editing
  class:actions-always={alwaysShowActions}
  class:no-meta={hideMetaRow}
  class="clip-card"
  style:--cpt={compact ? `${compactPaddingTop}px` : undefined}
  style:--cpb={compact ? `${compactPaddingBottom}px` : undefined}
  style:--cg={compact ? `${compactCardGap}px` : undefined}
  style:--cbr={compact ? `${compactCardBorderRadius}px` : undefined}
  style:--max-text-lines={`${showSecondaryText ? maxTextLines : 1}`}
  style:--compact-image-preview-height={compact && compactCardHeight
    ? `${Math.max(24, compactCardHeight - compactPaddingTop - compactPaddingBottom - 4 - (hideMetaRow ? 0 : 24))}px`
    : undefined}
  style:height={editing
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
              <button
                type="button"
                class="image-fullscreen-btn"
                onclick={(e) => {
                  e.stopPropagation();
                  onimagefullscreen?.(item.id);
                }}
                aria-label={_t("general.imageFullscreenButton")}
              >
                <AppIcon name="maximize" size={14} strokeWidth={2} />
              </button>
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
        {#if item.customTitle}
          <div class="text-preview custom-title">{item.title}</div>
          {#if secondaryPreviewText}
            <div class="content-preview">{secondaryPreviewText}</div>
          {/if}
        {:else}
          <div class="text-preview">{primaryFirstLine}</div>
          {#if showSecondaryText && primaryRestLines}
            <div class="content-preview">{primaryRestLines}</div>
          {/if}
        {/if}
      {/if}
    </div>

    {#if !hideMetaRow}
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
            aria-haspopup={action.actionType === "viewDate" ? "dialog" : undefined}
            aria-expanded={action.actionType === "viewDate"
              ? dateView?.isoDate === action.payload
              : undefined}
            onclick={(event) => handleAction(event, action)}
          >
            {#if quickActionKind(action) === "url"}
              <AppIcon name="globe" size={16} />
            {:else if quickActionKind(action) === "email"}
              <AppIcon name="mail" size={16} />
            {:else if quickActionKind(action) === "phone"}
              <AppIcon name="phone" size={16} />
            {:else if quickActionKind(action) === "date"}
              <AppIcon name="calendar" size={16} />
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
            ><AppIcon name="restore" size={16} /></button
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
      {#if index < 9}
        <span class="shortcut" class:shortcut-resident={quickCopyBadgeAlwaysVisible}
          >⌘{index + 1}</span
        >
      {/if}
    </div>
    {/if}
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

<dialog
  bind:this={dateDialog}
  class="date-action-dialog"
  aria-label={dateView?.label ?? "View date"}
  onclose={() => {
    dateView = null;
  }}
  onclick={handleDateDialogClick}
  onkeydown={handleDateDialogKeydown}
>
  {#if dateView}
    <div class="date-action-content">
      <span class="date-action-icon"><AppIcon name="calendar" size={20} /></span>
      <div class="date-action-text">
        <time datetime={dateView.isoDate}>{dateView.formattedDate}</time>
        <span>{dateView.isoDate}</span>
      </div>
      <button
        type="button"
        class="date-action-close"
        title="Close date"
        aria-label="Close date"
        onclick={closeDateDialog}
      >
        <AppIcon name="x" size={15} />
      </button>
    </div>
  {/if}
</dialog>

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
    padding: 0px 14px 0px 12px;
    border: 1px solid transparent;
    border-radius: 10px;
    color: var(--text-primary);
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
    font-size: 12px;
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
    border: 1.5px solid var(--text-faint);
    border-radius: 5px;
    color: transparent;
    background: transparent;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  .card-checkbox input:checked + .check-mark {
    border-color: var(--selection-color);
    background: var(--selection-color);
    color: #fff;
  }

  .clip-card:hover,
  .clip-card:focus-within {
    border-color: rgba(255, 255, 255, 0.035);
    background: var(--hover-bg);
  }

  .clip-card.selected:not(.checked) {
    border-color: rgba(255, 255, 255, 0.035);
    background: var(--hover-bg);
  }

  .clip-card.selected.checked {
    border-color: rgba(255, 255, 255, 0.035);
    background: color-mix(in srgb, var(--selection-color) 18%, var(--hover-bg));
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

  .clip-card.no-meta .content {
    padding-right: 0;
  }

  .text-preview {
    overflow: hidden;
    display: block;
    white-space: nowrap;
    text-overflow: ellipsis;
    max-width: 95%;
    color: var(--text-primary);
    font-size: var(--font-size-cardTitle, 13px);
    line-height: 1.55;
  }

  .content-preview {
    margin-top: 4px;
    overflow: hidden;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: var(--max-text-lines, 3);
    line-clamp: var(--max-text-lines, 3);
    color: var(--text-muted);
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
    color: var(--text-secondary);
  }

  .image-preview {
    position: relative;
    width: min(100%, 380px);
    height: 90px;
    overflow: hidden;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    background: var(--input-bg);
    box-shadow: inset 0 0 40px rgba(0, 0, 0, 0.3);
  }

  .clip-card.compact .image-preview {
    height: var(--compact-image-preview-height, 90px);
  }

  .image-preview img {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: contain;
    border-radius: 6px;
  }

  .image-fullscreen-btn {
    position: absolute;
    top: 6px;
    right: 6px;
    z-index: 2;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 5px;
    color: var(--text-secondary);
    background: rgba(0, 0, 0, 0.55);
    backdrop-filter: blur(4px);
    cursor: pointer;
    opacity: 0;
    transition:
      opacity 150ms ease,
      background 150ms ease;
  }

  .image-preview:hover .image-fullscreen-btn {
    opacity: 1;
  }

  .image-fullscreen-btn:hover {
    color: var(--text-primary);
    background: rgba(0, 0, 0, 0.75);
  }

  .clip-card.compact .image-placeholder {
    height: var(--compact-image-preview-height, 82px);
  }

  .image-placeholder {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 82px;
    color: var(--text-faint);
  }

  .meta-row {
    position: relative;
    z-index: 1;
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-wrap: nowrap;
    gap: 8px;
    min-width: 0;
    margin-top: 5px;
    overflow: hidden;
    color: var(--text-muted);
    font-size: var(--font-size-secondary, 11.5px);
    white-space: nowrap;
    pointer-events: none;
  }

  .meta-row > span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .source-mark {
    display: inline-flex;
    flex: 0 0 16px;
    align-items: center;
    justify-content: center;
    width: 16px;
    height: 16px;
    overflow: visible;
    color: var(--warning-color);
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
    color: var(--text-muted);
  }
  .file-count {
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .actions {
    display: flex;
    flex: 0 0 auto;
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
  .clip-card.selected .shortcut,
  .clip-card.actions-always .actions,
  .clip-card.actions-always .shortcut,
  .clip-card .shortcut.shortcut-resident {
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
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
  }

  .actions button:hover,
  .actions button.active {
    color: var(--warning-color);
    background: var(--hover-bg);
  }

  .shortcut {
    flex: 0 0 auto;
    margin-left: 8px;
    overflow: visible;
    color: var(--text-faint);
    font-size: 11.5px;
    pointer-events: none;
    opacity: 0;
    transition: opacity 120ms ease;
  }

  .date-action-dialog {
    position: fixed;
    width: min(320px, calc(100vw - 32px));
    margin: auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-primary);
    background: var(--card-bg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .date-action-dialog::backdrop {
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(2px);
  }

  .date-action-content {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 58px;
    padding: 12px 38px 12px 14px;
  }

  .date-action-icon {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--warning-color);
    background: var(--input-bg);
  }

  .date-action-text {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 3px;
  }

  .date-action-text time {
    color: var(--text-primary);
    font-size: 13px;
    line-height: 1.35;
  }

  .date-action-text span {
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
    font-size: 11px;
    line-height: 1.3;
  }

  .date-action-close {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
  }

  .date-action-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
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
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg);
    font: inherit;
    font-size: 12px;
    outline: none;
    box-sizing: border-box;
  }

  .edit-title-input:focus {
    border-color: var(--text-faint);
  }

  .edit-area textarea {
    width: 100%;
    box-sizing: border-box;
    padding: 10px 12px;
    border: 1px solid var(--selection-color);
    border-radius: 7px;
    color: var(--text-primary);
    background: var(--input-bg);
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
    border: 1px solid var(--border-color);
    border-radius: 5px;
    font: inherit;
    font-size: 11.5px;
    cursor: pointer;
    transition:
      background 100ms ease,
      border-color 100ms ease;
  }

  .edit-save {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .edit-save:hover {
    color: #fff;
    background: var(--border-color);
    border-color: var(--text-faint);
  }

  .edit-save-as-new {
    color: var(--text-muted);
    background: var(--card-bg);
    border-color: var(--border-color);
  }

  .edit-save-as-new:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .edit-save-as-new:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .edit-cancel {
    color: var(--text-muted);
    background: var(--card-bg);
  }

  .edit-cancel:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
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
