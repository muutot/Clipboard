<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import TagChip from "$lib/components/TagChip.svelte";
  import type { ClipboardItem, IconName } from "$lib/types/clipboard";
  import { messages, resolvePath } from "$lib/i18n";
  import { writeClipboardText } from "$lib/services/clipboard";
  import {
    extractEmails,
    extractUrls,
    extractPhones,
    extractColors,
    extractDates,
  } from "$lib/utils/patterns";

  interface Props {
    item: ClipboardItem;
    kindLabel: string;
    tagColors?: Record<string, string>;
    onsavetags: (id: string, tags: string[]) => void;
  }

  let { item, kindLabel, tagColors = {}, onsavetags }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  let tagDraft = $state("");
  let selectedFileIndex = $state(0);
  let activeMarkerFilter = $state<string | null>(null);

  function copyText(text: string) {
    void writeClipboardText(text).catch((err) => console.error("Copy to clipboard failed:", err));
  }

  function addTagFromInput() {
    const value = tagDraft.trim();
    if (!value) return;
    const current = item.tags ?? [];
    if (current.some((t) => t === value)) {
      tagDraft = "";
      return;
    }
    onsavetags(item.id, [...current, value]);
    tagDraft = "";
  }

  function removeTag(tag: string) {
    onsavetags(
      item.id,
      (item.tags ?? []).filter((t) => t !== tag),
    );
  }

  function formatDateTime(ts: number): string {
    return new Date(ts).toLocaleString();
  }

  function formatMetadataTime(ts: number | undefined): string {
    return ts === undefined ? _t("detail.unknown") : formatDateTime(ts);
  }

  function formatMetadataJson(metadataJson: string | null | undefined): string {
    if (!metadataJson) return "";
    try {
      const obj = JSON.parse(metadataJson);
      delete (obj as Record<string, unknown>)["clipboardFormats"];
      return JSON.stringify(obj, null, 2);
    } catch {
      return metadataJson;
    }
  }

  function getOcrStatusLabel(status: NonNullable<ClipboardItem["ocrStatus"]>): string {
    const map: Record<NonNullable<ClipboardItem["ocrStatus"]>, string> = {
      pending: _t("detail.pending"),
      processing: _t("detail.processing"),
      completed: _t("detail.completed"),
      failed: _t("detail.failed"),
      none: _t("detail.noOcr"),
    };
    return map[status];
  }

  const specialMarkers = $derived.by(() => {
    const text = item.textContent || [item.title, item.preview].filter(Boolean).join(" ");
    return {
      emails: extractEmails(text),
      urls: extractUrls(text),
      phones: extractPhones(text),
      colors: extractColors(text),
      dates: extractDates(text),
    };
  });
  const emails = $derived(specialMarkers.emails);
  const urls = $derived(specialMarkers.urls);
  const phones = $derived(specialMarkers.phones);
  const colors = $derived(specialMarkers.colors);
  const dates = $derived(specialMarkers.dates);
  const hasSpecialMarkers = $derived(
    emails.length > 0 ||
      urls.length > 0 ||
      phones.length > 0 ||
      colors.length > 0 ||
      dates.length > 0,
  );

  const markerGroups = $derived.by(() => {
    const groups: { kind: string; label: string; icon: IconName; items: string[] }[] = [];
    if (emails.length > 0)
      groups.push({ kind: "email", label: _t("detail.markerEmails"), icon: "mail", items: emails });
    if (urls.length > 0)
      groups.push({ kind: "url", label: _t("detail.markerLinks"), icon: "globe", items: urls });
    if (phones.length > 0)
      groups.push({
        kind: "phone",
        label: _t("detail.markerPhones"),
        icon: "phone",
        items: phones,
      });
    if (colors.length > 0)
      groups.push({
        kind: "color",
        label: _t("detail.markerColors"),
        icon: "palette",
        items: colors,
      });
    return groups;
  });
  const showMarkerFilters = $derived(markerGroups.length > 1);

  const resourceMetadata = $derived(item.resourceMetadata);
  const resourceFiles = $derived(item.fileMeta ?? []);
  const rawMetadata = $derived(formatMetadataJson(item.metadataJson));

  $effect(() => {
    void item.id;
    selectedFileIndex = 0;
  });
</script>

<div class="details-section">
  <dl class="detail-list">
    <div class="detail-row">
      <dt><AppIcon name="info" size={14} /> {_t("detail.sourceApp")}</dt>
      <dd>{item.sourceApp}</dd>
    </div>
    <div class="detail-row tags-row">
      <dt><AppIcon name="tag" size={14} /> {_t("detail.tags")}</dt>
      <dd class="tags-editor">
        {#each item.tags ?? [] as tag (tag)}
          <TagChip
            {tag}
            accent={tagColors[tag]}
            onremove={removeTag}
            removeAriaLabel={_t("detail.removeTag")}
          />
        {/each}
        <span class="tag-input-wrap">
          <input
            bind:value={tagDraft}
            placeholder={_t("detail.addTagPlaceholder")}
            onkeydown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                addTagFromInput();
              }
            }}
          />
          <button
            type="button"
            class="tag-add"
            aria-label={_t("detail.addTag")}
            disabled={!tagDraft.trim()}
            onclick={addTagFromInput}><AppIcon name="plus" size={12} /></button
          >
        </span>
      </dd>
    </div>
    <div class="detail-row">
      <dt><AppIcon name="file" size={14} /> {_t("detail.contentType")}</dt>
      <dd>{kindLabel}</dd>
    </div>
    <div class="detail-row">
      <dt><AppIcon name="clock" size={14} /> {_t("detail.copyTime")}</dt>
      <dd>{formatDateTime(item.createdAt)}</dd>
    </div>
    <div class="detail-row">
      <dt><AppIcon name="ruler" size={14} /> {_t("detail.size")}</dt>
      <dd>{item.sizeLabel}</dd>
    </div>
    {#if item.mimeType}
      <div class="detail-row">
        <dt><AppIcon name="mime" size={14} /> {_t("detail.mimeInfo")}</dt>
        <dd><code>{item.mimeType}</code></dd>
      </div>
    {/if}
    {#if resourceMetadata?.extension}
      <div class="detail-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.extension")}</dt>
        <dd><code>.{resourceMetadata.extension}</code></dd>
      </div>
    {/if}
    {#if item.fileName && (item.kind === "image" || item.kind === "file")}
      <div class="detail-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.fileName")}</dt>
        <dd>{item.fileName}</dd>
      </div>
    {/if}
    {#if item.ocrStatus && item.ocrStatus !== "none"}
      <div class="detail-row">
        <dt><AppIcon name="scan" size={14} /> {_t("detail.ocrStatus")}</dt>
        <dd
          class="ocr-badge"
          class:ocr-completed={item.ocrStatus === "completed"}
          class:ocr-pending={item.ocrStatus === "pending" || item.ocrStatus === "processing"}
          class:ocr-failed={item.ocrStatus === "failed"}
        >
          {getOcrStatusLabel(item.ocrStatus)}
        </dd>
      </div>
    {/if}
    {#if item.kind === "image" && item.imageMeta}
      <div class="detail-row">
        <dt><AppIcon name="image" size={14} /> {_t("detail.dimensions")}</dt>
        <dd>{item.imageMeta.width} × {item.imageMeta.height}</dd>
      </div>
    {/if}
    {#if item.kind === "file" && resourceFiles.length > 0}
      <div class="detail-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.fileCount")}</dt>
        <dd>{resourceFiles.length}</dd>
      </div>
      {#if resourceFiles.length > 1}
        <div class="detail-row">
          <dt><AppIcon name="file" size={14} /> {_t("detail.file")}</dt>
          <dd>
            <CustomSelect
              className="file-selector"
              value={selectedFileIndex}
              options={resourceFiles.map((file, index) => ({
                value: index,
                label: file.name,
              }))}
              onchange={(v) => (selectedFileIndex = v as number)}
            />
          </dd>
        </div>
      {/if}
      {#if selectedFileIndex < resourceFiles.length}
        {@const file = resourceFiles[selectedFileIndex]}
        {#if file.originalPath}
          <div class="detail-row path-row">
            <dt><AppIcon name="file" size={14} /> {_t("detail.originalPath")}</dt>
            <dd class="path-value"><code>{file.originalPath}</code></dd>
          </div>
        {/if}
        {#if file.contentHash}
          <div class="detail-row path-row">
            <dt><AppIcon name="info" size={14} /> {_t("detail.contentHash")}</dt>
            <dd class="path-value"><code>{file.contentHash}</code></dd>
          </div>
        {/if}
        <div class="detail-row">
          <dt><AppIcon name="calendar" size={14} /> {_t("detail.createdTime")}</dt>
          <dd>{formatMetadataTime(file.createdAtMs)}</dd>
        </div>
        <div class="detail-row">
          <dt><AppIcon name="edit" size={14} /> {_t("detail.modifiedTime")}</dt>
          <dd>{formatMetadataTime(file.modifiedAtMs)}</dd>
        </div>
        <div class="detail-row">
          <dt><AppIcon name="eye" size={14} /> {_t("detail.accessedTime")}</dt>
          <dd>{formatMetadataTime(file.accessedAtMs)}</dd>
        </div>
        <div class="detail-row">
          <dt><AppIcon name="clock" size={14} /> {_t("detail.readOnly")}</dt>
          <dd>
            {file.readOnly === undefined
              ? _t("detail.unknown")
              : file.readOnly
                ? _t("detail.yes")
                : _t("detail.no")}
          </dd>
        </div>
        <div class="detail-row">
          <dt><AppIcon name="file" size={14} /> {_t("detail.directory")}</dt>
          <dd>
            {file.isDirectory === undefined
              ? _t("detail.unknown")
              : file.isDirectory
                ? _t("detail.yes")
                : _t("detail.no")}
          </dd>
        </div>
      {/if}
    {/if}
    {#if item.kind === "image" && (item.resourcePath || resourceMetadata?.resourcePath)}
      <div class="detail-row path-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.resourcePath")}</dt>
        <dd class="path-value">
          <code>{resourceMetadata?.resourcePath ?? item.resourcePath}</code>
        </dd>
      </div>
    {/if}
    {#if item.kind === "image" && resourceMetadata?.storagePath && resourceMetadata.storagePath !== resourceMetadata.resourcePath}
      <div class="detail-row path-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.storagePath")}</dt>
        <dd class="path-value"><code>{resourceMetadata.storagePath}</code></dd>
      </div>
    {/if}
    {#if item.kind === "image" && resourceMetadata?.previewPath && resourceMetadata.previewPath !== resourceMetadata.resourcePath}
      <div class="detail-row path-row">
        <dt><AppIcon name="image" size={14} /> {_t("detail.previewPath")}</dt>
        <dd class="path-value"><code>{resourceMetadata.previewPath}</code></dd>
      </div>
    {/if}
    {#if item.kind === "image" && resourceMetadata?.originalPath}
      <div class="detail-row path-row">
        <dt><AppIcon name="file" size={14} /> {_t("detail.originalPath")}</dt>
        <dd class="path-value"><code>{resourceMetadata.originalPath}</code></dd>
      </div>
    {/if}
    {#if item.kind === "image" && resourceMetadata?.contentHash}
      <div class="detail-row">
        <dt><AppIcon name="info" size={14} /> {_t("detail.contentHash")}</dt>
        <dd class="path-value"><code>{resourceMetadata.contentHash}</code></dd>
      </div>
    {/if}
  </dl>

  {#if rawMetadata}
    <details class="raw-metadata">
      <summary>{_t("detail.rawMetadata")}</summary>
      <pre>{rawMetadata}</pre>
    </details>
  {/if}

  {#if hasSpecialMarkers}
    <div class="special-section">
      <strong class="special-title">{_t("detail.specialMarkers")}</strong>
      {#if showMarkerFilters}
        <div class="marker-filters">
          <button
            type="button"
            class="marker-filter-btn"
            class:active={activeMarkerFilter === null}
            onclick={() => (activeMarkerFilter = null)}
          >
            {_t("filter.all")}
          </button>
          {#each markerGroups as group (group.kind)}
            <button
              type="button"
              class="marker-filter-btn"
              class:active={activeMarkerFilter === group.kind}
              onclick={() =>
                (activeMarkerFilter = activeMarkerFilter === group.kind ? null : group.kind)}
            >
              <AppIcon name={group.icon} size={12} />
              {group.label} ({group.items.length})
            </button>
          {/each}
        </div>
      {/if}
      <div class="markers-list">
        {#each markerGroups as group (group.kind)}
          {#if activeMarkerFilter === null || activeMarkerFilter === group.kind}
            {#if group.kind === "color"}
              {#each group.items as item}
                <div class="marker-item color-marker">
                  <span class="color-swatch" style="background:{item}"></span>
                  <code>{item}</code>
                  <button type="button" onclick={() => copyText(item)}>
                    <AppIcon name="copy" size={11} />
                  </button>
                </div>
              {/each}
            {:else if group.kind === "url"}
              {#each group.items as item}
                <div class="marker-item">
                  <AppIcon name="globe" size={13} />
                  <a href={item} target="_blank" rel="noopener noreferrer">{item}</a>
                  <button type="button" onclick={() => copyText(item)}>
                    <AppIcon name="copy" size={11} />
                  </button>
                </div>
              {/each}
            {:else}
              {#each group.items as item}
                <div class="marker-item">
                  <AppIcon name={group.icon} size={13} />
                  <span>{item}</span>
                  <button type="button" onclick={() => copyText(item)}>
                    <AppIcon name="copy" size={11} />
                  </button>
                </div>
              {/each}
            {/if}
          {/if}
        {/each}
      </div>
    </div>
  {/if}
</div>

<style>
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
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    overflow: hidden;
    background: var(--border-subtle);
  }

  .detail-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 10px 14px;
    background: var(--input-bg);
    gap: 12px;
  }

  .detail-row.tags-row {
    align-items: flex-start;
  }

  .tags-editor {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px;
    justify-content: flex-end;
    max-width: 65%;
  }

  .tag-input-wrap {
    display: inline-flex;
    align-items: center;
    gap: 2px;
  }

  .tag-input-wrap input {
    width: 90px;
    padding: 2px 6px;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-primary);
    background: var(--surface-bg);
    font: inherit;
    font-size: 11px;
    outline: none;
  }

  .tag-input-wrap input:focus {
    border-color: var(--text-faint);
  }

  .tag-add {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--surface-bg);
    cursor: pointer;
  }

  .tag-add:hover:not(:disabled) {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .tag-add:disabled {
    opacity: 0.35;
    cursor: default;
  }

  .detail-row dt {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 500;
    flex-shrink: 0;
  }

  .detail-row dd {
    margin: 0;
    color: var(--text-primary);
    font-size: 11px;
    text-align: right;
    word-break: break-all;
  }

  .detail-row dd code {
    padding: 2px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 10px;
  }

  .detail-row.path-row {
    align-items: flex-start;
  }

  .detail-row .path-value {
    min-width: 0;
    max-width: 68%;
    text-align: left;
  }

  .detail-row .path-value code {
    display: block;
    max-width: 100%;
    white-space: normal;
    overflow-wrap: anywhere;
    word-break: break-word;
  }

  .raw-metadata pre {
    max-height: 360px;
    margin: 0;
    padding: 10px 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      11px/1.55 "Cascadia Code",
      Consolas,
      monospace;
    white-space: pre-wrap;
    overflow: auto;
    overflow-wrap: anywhere;
  }

  .raw-metadata {
    border: 1px solid var(--border-subtle);
    border-radius: 7px;
    background: var(--input-bg);
  }

  .raw-metadata summary {
    padding: 9px 12px;
    color: var(--text-muted);
    font-size: 10px;
    cursor: pointer;
  }

  .raw-metadata pre {
    max-height: 280px;
    margin: 0 8px 8px;
  }

  .ocr-badge {
    padding: 2px 8px;
    border-radius: 999px;
    font-size: 10px;
  }

  .ocr-badge.ocr-completed {
    border: 1px solid color-mix(in srgb, var(--success-color) 40%, transparent);
    color: color-mix(in srgb, var(--success-color) 75%, white);
    background: color-mix(in srgb, var(--success-color) 12%, transparent);
  }

  .ocr-badge.ocr-pending {
    border: 1px solid color-mix(in srgb, var(--warning-color) 40%, transparent);
    color: color-mix(in srgb, var(--warning-color) 75%, white);
    background: color-mix(in srgb, var(--warning-color) 12%, transparent);
  }

  .ocr-badge.ocr-failed {
    border: 1px solid color-mix(in srgb, var(--danger-color) 40%, transparent);
    color: color-mix(in srgb, var(--danger-color) 75%, white);
    background: color-mix(in srgb, var(--danger-color) 12%, transparent);
  }

  .special-section {
    padding: 12px;
    border: 1px solid var(--border-subtle);
    border-radius: 8px;
    background: var(--input-bg);
  }

  .special-title {
    display: block;
    margin-bottom: 10px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 560;
  }

  .markers-list {
    display: grid;
    gap: 6px;
    max-height: 260px;
    overflow-y: auto;
  }

  .marker-filters {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin-bottom: 10px;
  }

  .marker-filter-btn {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 4px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: 14px;
    font-size: 10px;
    font-weight: 500;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
    transition: all 120ms ease;
  }

  .marker-filter-btn:hover {
    border-color: var(--selection-color);
    color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 8%, transparent);
  }

  .marker-filter-btn.active {
    border-color: var(--border-subtle);
    color: var(--selection-color);
    background: color-mix(in srgb, var(--selection-color) 14%, transparent);
  }

  /* Marker group chips: suppress the blue focus outline (no ring, consistent
     with the group tabs). A non-active focused chip still gets a background
     highlight so keyboard focus stays visible. */
  .marker-filter-btn:focus-visible {
    outline: none;
  }
  .marker-filter-btn:not(.active):focus-visible {
    background: var(--hover-bg);
    color: var(--text-primary);
  }

  .marker-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 10px;
    border: 1px solid var(--border-subtle);
    border-radius: 6px;
    color: var(--text-secondary);
    font-size: 11px;
    background: var(--input-bg);
  }

  .marker-item span,
  .marker-item a {
    flex: 1;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    min-width: 0;
  }

  :global(.file-selector) {
    width: 100%;
  }

  .marker-item a {
    color: var(--selection-color);
    text-decoration: none;
    white-space: normal;
    word-break: break-all;
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
    color: var(--text-faint);
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    transition: color 100ms ease;
  }

  .marker-item button:hover {
    color: var(--text-secondary);
  }

  .color-swatch {
    width: 16px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid var(--border-color);
    flex-shrink: 0;
  }

  .color-marker code {
    padding: 2px 6px;
    border: 1px solid var(--border-subtle);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--input-bg);
    font:
      10px "Cascadia Code",
      Consolas,
      monospace;
  }
</style>
