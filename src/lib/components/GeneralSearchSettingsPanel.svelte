<script lang="ts">
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import SettingEntry from "$lib/components/SettingEntry.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import type { SearchSuggestionMode, SortRule } from "$lib/types/clipboard";
  import { generalSettings } from "$lib/services/settings";
  import { onDestroy } from "svelte";
  import type { SettingEntryConfig } from "$lib/types/settings-entry";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  let sortDragIdx = $state<number | null>(null);
  let sortListEl = $state<HTMLDivElement | null>(null);
  let stopPointerDrag: (() => void) | undefined;

  const ALL_SORT_FIELDS: SortRule["field"][] = [
    "createdAt",
    "lastUsedAt",
    "title",
    "size",
    "kind",
    "favorite",
  ];

  const SORT_FIELD_LABELS: Record<SortRule["field"], string> = {
    createdAt: "general.sortFieldCreatedAt",
    lastUsedAt: "general.sortFieldLastUsedAt",
    title: "general.sortFieldTitle",
    size: "general.sortFieldSize",
    kind: "general.sortFieldKind",
    favorite: "general.sortFieldFavorite",
  };

  function pointerDragStart(idx: number, _e: PointerEvent) {
    stopPointerDrag?.();
    sortDragIdx = idx;
    const rows = sortListEl?.querySelectorAll<HTMLElement>(".sort-rule-row");

    function onMove(ev: PointerEvent) {
      if (!rows || rows.length === 0) return;
      let target: number | null = null;
      for (let i = 0; i < rows.length; i++) {
        const rect = rows[i].getBoundingClientRect();
        if (ev.clientY >= rect.top && ev.clientY <= rect.bottom) {
          target = i;
          break;
        }
      }
      for (let i = 0; i < rows.length; i++) {
        if (target !== null && i === target && i !== sortDragIdx) {
          rows[i].classList.add("sort-drag-over");
        } else {
          rows[i].classList.remove("sort-drag-over");
        }
      }
    }

    function cleanup() {
      rows?.forEach((row) => row.classList.remove("sort-drag-over"));
      document.removeEventListener("pointermove", onMove);
      document.removeEventListener("pointerup", onUp);
      if (stopPointerDrag === cleanup) stopPointerDrag = undefined;
    }

    function onUp() {
      const target = [...(rows ?? [])].findIndex((r) => r.classList.contains("sort-drag-over"));
      if (target !== -1 && target !== sortDragIdx) {
        moveSortRule(sortDragIdx!, target);
      }
      sortDragIdx = null;
      cleanup();
    }

    stopPointerDrag = cleanup;
    document.addEventListener("pointermove", onMove);
    document.addEventListener("pointerup", onUp);
  }

  function moveSortRule(fromIdx: number, toIdx: number) {
    if (fromIdx === toIdx) return;
    const newRules = [...s.searchSortRules];
    const [removed] = newRules.splice(fromIdx, 1);
    newRules.splice(toIdx, 0, removed);
    generalSettings.updateSetting("searchSortRules", newRules);
  }

  function addSortRule() {
    const used = new Set(s.searchSortRules.map((r: SortRule) => r.field));
    const field = (ALL_SORT_FIELDS.find((f) => !used.has(f)) ?? "createdAt") as SortRule["field"];
    generalSettings.updateSetting("searchSortRules", [
      ...s.searchSortRules,
      { field, direction: "desc" },
    ]);
  }

  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  onDestroy(() => {
    stopPointerDrag?.();
  });

  const searchEntries: SettingEntryConfig[] = $derived([
    {
      type: "select",
      icon: "search",
      label: _t("general.searchSuggestionMode"),
      desc: _t("general.searchSuggestionModeDescription"),
      get: () => s.searchSuggestionMode,
      options: [
        { value: "off", label: _t("general.searchSuggestionOff") },
        { value: "panel", label: _t("general.searchSuggestionPanel") },
        { value: "inline", label: _t("general.searchSuggestionInline") },
      ],
      set: (v) => generalSettings.updateSetting("searchSuggestionMode", v as SearchSuggestionMode),
    },
    {
      type: "toggle",
      icon: "clock",
      label: _t("general.searchHistory"),
      desc: _t("general.searchHistoryDescription"),
      get: () => s.searchHistoryEnabled,
      set: (v) => generalSettings.updateSetting("searchHistoryEnabled", v),
    },
    {
      type: "text",
      icon: "search",
      label: _t("general.searchPlaceholder"),
      desc: _t("general.searchPlaceholderDescription"),
      get: () => s.searchPlaceholder,
      set: (v) => generalSettings.updateSetting("searchPlaceholder", v),
      maxlength: 80,
      placeholder: _t("app.searchPlaceholder"),
      actionLabel: _t("general.searchPlaceholderDefault"),
      actionVisible: () => s.searchPlaceholder.trim().length > 0,
      onaction: () => generalSettings.updateSetting("searchPlaceholder", ""),
    },
    {
      type: "custom",
      variant: "column",
      id: "general.search-sort-rules",
      icon: "sliders",
      label: _t("general.searchSortRules"),
      desc: _t("general.searchSortRulesDescription"),
      actionLabel: () => `+ ${_t("general.sortAddRule")}`,
      actionVisible: () =>
        s.searchSortRules.length < 3 && s.searchSortRules.length < ALL_SORT_FIELDS.length,
      onaction: addSortRule,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchPageSizeLimit"),
      desc: _t("general.searchPageSizeLimitDescription"),
      get: () => s.searchPageSizeLimit,
      set: (v) => generalSettings.updateSetting("searchPageSizeLimit", v),
      min: 50,
      max: 1000,
      step: 50,
      suffix: ` ${_t("general.searchPageSizeLimitUnit")}`,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchPageSize"),
      desc: _t("general.searchPageSizeDescription"),
      get: () => Math.min(s.display.searchPageSize, s.searchPageSizeLimit),
      set: (v) =>
        generalSettings.updateSetting("display", {
          ...s.display,
          searchPageSize: Math.min(v, s.searchPageSizeLimit),
        }),
      min: 50,
      max: () => Math.min(s.searchPageSizeLimit, 500),
      step: 50,
      suffix: ` ${_t("general.searchPageSizeUnit")}`,
    },
    {
      type: "slider",
      icon: "search",
      label: _t("general.searchCacheSize"),
      desc: _t("general.searchCacheSizeDescription"),
      get: () => s.searchCacheSize,
      set: (v) => generalSettings.updateSetting("searchCacheSize", v),
      min: 200,
      max: 2000,
      step: 50,
      suffix: ` ${_t("general.searchCacheSizeUnit")}`,
    },
    {
      type: "select",
      icon: "sliders",
      label: _t("general.searchCacheEviction"),
      desc: _t("general.searchCacheEvictionDescription"),
      get: () => s.searchCacheEviction,
      options: [
        { value: "fifo", label: _t("general.searchCacheEvictionFifo") },
        { value: "lru", label: _t("general.searchCacheEvictionLru") },
      ],
      set: (v) => generalSettings.updateSetting("searchCacheEviction", v as "fifo" | "lru"),
    },
    {
      type: "select",
      icon: "search",
      label: _t("general.searchIndexSyncMode"),
      desc: _t("general.searchIndexSyncModeDescription"),
      get: () => s.searchIndexSyncMode,
      options: [
        { value: "lazy", label: _t("general.searchIndexSyncModeLazy") },
        { value: "background", label: _t("general.searchIndexSyncModeBackground") },
      ],
      set: (v) => generalSettings.updateSetting("searchIndexSyncMode", v as "lazy" | "background"),
    },
  ]);
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("general.eyebrow")}</span>
      <h2>{_t("storage.generalSearchTab")}</h2>
      <p>{_t("storage.generalSearchDescription")}</p>
    </div>
    {#if s.showSettingsCloseButton}
      <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
        >×</button
      >
    {/if}
  </header>
{/if}

<div class="settings-scroll">
  {#each searchEntries as config}
    <SettingEntry {config}
      >{#snippet children()}
        {#if config.type === "custom" && config.id === "general.search-sort-rules"}
          <div class="sort-rules-list" role="list" bind:this={sortListEl}>
            {#each s.searchSortRules as rule, idx (idx)}
              <div class="sort-rule-row" class:sort-dragging={sortDragIdx === idx} role="listitem">
                <span
                  class="sort-grip"
                  role="button"
                  tabindex="0"
                  aria-label={_t("general.sortDragHandle")}
                  onpointerdown={(e) => pointerDragStart(idx, e)}
                >
                  <span class="grip-dot"></span>
                  <span class="grip-dot"></span>
                  <span class="grip-dot"></span>
                  <span class="grip-dot"></span>
                </span>
                <CustomSelect
                  className="sort-field-select"
                  value={rule.field}
                  ariaLabel={_t("general.searchSortRules")}
                  options={ALL_SORT_FIELDS.map((f) => ({
                    value: f,
                    label: _t(SORT_FIELD_LABELS[f]),
                    disabled: s.searchSortRules.some(
                      (r: SortRule, i: number) => i !== idx && r.field === f,
                    ),
                  }))}
                  onchange={(v) => {
                    const newRules = [...s.searchSortRules];
                    newRules[idx] = { ...rule, field: v as SortRule["field"] };
                    generalSettings.updateSetting("searchSortRules", newRules);
                  }}
                />
                <button
                  type="button"
                  class="sort-direction-btn"
                  title={rule.direction === "asc" ? _t("general.sortAsc") : _t("general.sortDesc")}
                  aria-label={rule.direction === "asc"
                    ? _t("general.sortAsc")
                    : _t("general.sortDesc")}
                  onclick={() => {
                    const newRules = [...s.searchSortRules];
                    newRules[idx] = {
                      ...rule,
                      direction: rule.direction === "asc" ? "desc" : ("asc" as const),
                    };
                    generalSettings.updateSetting("searchSortRules", newRules);
                  }}
                >
                  {rule.direction === "asc" ? "↑" : "↓"}
                </button>
                {#if s.searchSortRules.length > 1}
                  <button
                    type="button"
                    class="sort-remove-btn"
                    title={_t("general.sortRemoveRule")}
                    aria-label={_t("general.sortRemoveRule")}
                    onclick={() => {
                      const newRules = s.searchSortRules.filter((_, i) => i !== idx);
                      generalSettings.updateSetting("searchSortRules", newRules);
                    }}>×</button
                  >
                {/if}
              </div>
            {/each}
          </div>
        {/if}
      {/snippet}
    </SettingEntry>
  {/each}
</div>

<style>
  .sort-rules-list {
    display: flex;
    flex-direction: column;
    gap: 6px;
    margin: 8px 0 0;
  }

  .sort-rule-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px 0;
    border-radius: var(--settings-control-radius, 6px);
  }

  .sort-rule-row.sort-dragging {
    opacity: 0.4;
  }

  :global(.sort-drag-over) {
    outline: 2px solid var(--selection-color) !important;
    outline-offset: 2px;
    border-radius: 6px;
    background: color-mix(in srgb, var(--selection-color) 15%, transparent);
  }

  .sort-grip {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 2px;
    padding: 4px;
    cursor: grab;
    flex-shrink: 0;
    border-radius: 4px;
    align-self: stretch;
    align-content: center;
    user-select: none;
    touch-action: none;
  }

  .sort-grip:hover {
    background: var(--hover-bg);
  }

  .sort-grip:active {
    cursor: grabbing;
  }

  .grip-dot {
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-muted);
    display: block;
  }

  :global(.sort-field-select) {
    flex: 1;
    min-width: 0;
  }

  .sort-direction-btn {
    width: 32px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: 14px;
    cursor: pointer;
    flex-shrink: 0;
  }

  .sort-direction-btn:hover {
    background: var(--hover-bg);
  }

  .sort-remove-btn {
    width: 28px;
    height: 28px;
    border-radius: var(--settings-control-radius, 6px);
    border: 1px solid var(--border-color);
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 16px;
    flex-shrink: 0;
  }

  .sort-remove-btn:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
  }
</style>
