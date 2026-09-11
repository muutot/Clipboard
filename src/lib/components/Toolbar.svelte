<script lang="ts">
  import { tick } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";
  import { alignDropdownOptionText } from "$lib/utils/dropdown";
  import type { ClipboardFilter, IconName } from "$lib/types/clipboard";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  export interface ToolbarFilter {
    id: ClipboardFilter;
    label: string;
    icon: IconName;
  }

  export interface DateFilterOption {
    id: string;
    label: string;
  }

  interface Props {
    filters: ToolbarFilter[];
    activeFilter: ClipboardFilter;
    filterShortcutBindings: Record<string, string[]>;
    onselectfilter: (id: ClipboardFilter) => void;
    sourceApps: string[];
    sourceAppFilter: string;
    onsourceapp: (app: string) => void;
    dateFilter: string;
    dateFilterOptions: DateFilterOption[];
    ondatefilter: (id: string) => void;
    onsettings: () => void;
  }

  let {
    filters,
    activeFilter,
    filterShortcutBindings,
    onselectfilter,
    sourceApps,
    sourceAppFilter,
    onsourceapp,
    dateFilter,
    dateFilterOptions,
    ondatefilter,
    onsettings,
  }: Props = $props();

  let sourceAppDropdownOpen = $state(false);
  let sourceAppSearch = $state("");
  let dateDropdownOpen = $state(false);
  let sourceAppDropdownEl: HTMLDivElement | undefined = $state();
  let dateDropdownEl: HTMLDivElement | undefined = $state();

  const filteredSourceApps = $derived(
    sourceAppSearch
      ? sourceApps.filter((a) => a.toLowerCase().includes(sourceAppSearch.toLowerCase()))
      : sourceApps,
  );

  $effect(() => {
    if (!sourceAppDropdownOpen) return;
    void filteredSourceApps;
    tick().then(() => {
      if (sourceAppDropdownEl) alignDropdownOptionText(sourceAppDropdownEl);
    });
  });

  $effect(() => {
    if (!dateDropdownOpen || !dateDropdownEl) return;
    const el = dateDropdownEl;
    tick().then(() => alignDropdownOptionText(el));
  });
</script>

<div
  class="toolbar"
  role="presentation"
  aria-label={_t("actions.dragWindow")}
  onmousedown={(e) => {
    if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
  }}
>
  <div class="filters" role="tablist" aria-label={_t("filter.all")}>
    {#each filters as filter}
      <button
        type="button"
        role="tab"
        tabindex={activeFilter === filter.id ? 0 : -1}
        aria-selected={activeFilter === filter.id}
        class:active={activeFilter === filter.id}
        title={filterShortcutBindings[filter.id]?.[0]
          ? `${filter.label} (${filterShortcutBindings[filter.id]?.[0]})`
          : filter.label}
        onclick={() => onselectfilter(filter.id)}
      >
        {#if $generalSettings.groupDisplayMode !== "textOnly"}
          <AppIcon
            name={filter.icon}
            size={16}
            filled={filter.id === "favorite" && activeFilter === filter.id}
          />
        {/if}
        {#if $generalSettings.groupDisplayMode !== "iconOnly"}
          <span>{filter.label}</span>
        {/if}
      </button>
    {/each}
  </div>

  <div class="toolbar-right">
    <div class="filter-dropdowns">
      <!-- Source app filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          class="filter-dropdown-btn"
          onclick={() => (sourceAppDropdownOpen = !sourceAppDropdownOpen)}
          aria-label={_t("sourceApp.all")}
          aria-haspopup="menu"
          aria-expanded={sourceAppDropdownOpen}
          title={_t("sourceApp.all")}
        >
          {#if !sourceAppFilter}
            <AppIcon name="filter" size={15} />
          {/if}
          <span class="dropdown-label">{sourceAppFilter || _t("sourceApp.all")}</span>
          {#if !sourceAppFilter}
            <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
          {/if}
        </button>
        {#if sourceAppDropdownOpen}
          <div class="dropdown-popover popover-surface" role="menu" bind:this={sourceAppDropdownEl}>
            <div
              class="dropdown-backdrop"
              onclick={() => (sourceAppDropdownOpen = false)}
              aria-hidden="true"
            ></div>
            <div class="dropdown-search">
              <AppIcon name="search" size={13} />
              <input
                type="text"
                bind:value={sourceAppSearch}
                placeholder={_t("sourceApp.placeholder")}
                autocomplete="off"
              />
            </div>
            <div class="dropdown-items">
              <button
                type="button"
                role="menuitem"
                class:selected={sourceAppFilter === ""}
                onclick={() => {
                  onsourceapp("");
                  sourceAppDropdownOpen = false;
                }}><span>{_t("sourceApp.all")}</span></button
              >
              {#each filteredSourceApps as app}
                <button
                  type="button"
                  role="menuitem"
                  class:selected={sourceAppFilter === app}
                  onclick={() => {
                    onsourceapp(app);
                    sourceAppDropdownOpen = false;
                  }}><span>{app}</span></button
                >
              {/each}
            </div>
          </div>
        {/if}
      </div>

      <!-- Date filter -->
      <div class="dropdown-wrapper">
        <button
          type="button"
          class="filter-dropdown-btn"
          onclick={() => (dateDropdownOpen = !dateDropdownOpen)}
          aria-label={_t("dateFilter.all")}
          aria-haspopup="menu"
          aria-expanded={dateDropdownOpen}
          title={_t("dateFilter.all")}
        >
          {#if dateFilter === "all"}
            <AppIcon name="calendar" size={15} />
          {/if}
          <span class="dropdown-label"
            >{dateFilter === "all"
              ? _t("dateFilter.all")
              : (dateFilterOptions.find((o) => o.id === dateFilter)?.label ??
                _t("dateFilter.all"))}</span
          >
          {#if dateFilter === "all"}
            <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
          {/if}
        </button>
        {#if dateDropdownOpen}
          <div class="dropdown-popover popover-surface" role="menu" bind:this={dateDropdownEl}>
            <div
              class="dropdown-backdrop"
              onclick={() => (dateDropdownOpen = false)}
              aria-hidden="true"
            ></div>
            {#each dateFilterOptions as option}
              <button
                type="button"
                role="menuitem"
                class:selected={dateFilter === option.id}
                onclick={() => {
                  ondatefilter(option.id);
                  dateDropdownOpen = false;
                }}><span>{option.label}</span></button
              >
            {/each}
          </div>
        {/if}
      </div>
    </div>

    <div class="toolbar-actions">
      <button
        type="button"
        class:active={$generalSettings.alwaysOnTop}
        aria-label={_t("toolbar.pinWindow")}
        title={_t("toolbar.pinWindow")}
        onclick={() => generalSettings.updateSetting("alwaysOnTop", !$generalSettings.alwaysOnTop)}
        ><AppIcon name="window-top" size={17} /></button
      >
      <button
        type="button"
        aria-label={_t("toolbar.settings")}
        title={_t("toolbar.settings")}
        onclick={onsettings}><AppIcon name="settings" size={17} /></button
      >
    </div>
  </div>
</div>

<style>
  .toolbar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 2px 8px 8px;
  }

  .filters,
  .filter-dropdowns,
  .toolbar-actions,
  .toolbar-right {
    display: flex;
    align-items: center;
  }

  .toolbar-right {
    gap: 8px;
    margin-left: auto;
    flex: 0 0 auto;
  }

  .filters {
    gap: 2px;
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: none;
  }

  .filters::-webkit-scrollbar {
    display: none;
  }

  .filters button,
  .toolbar-actions button {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    border: 0;
    color: var(--text-secondary);
    background: transparent;
    cursor: pointer;
  }

  .filters button {
    gap: 5px;
    height: 31px;
    padding: 3px 8px;
    border-radius: 6px;
    font-size: 12px;
  }

  .filters button:hover,
  .filters button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .filters button.active:first-child {
    color: var(--selection-color);
  }

  /* Group tabs use a roving tabindex, so the focused tab is always the active
     one; its .active background already indicates state. Suppress the focus
     outline so mouse-click and keyboard selection look identical (no ring). */
  .filters button:focus-visible {
    outline: none;
  }

  .filters button:nth-child(2) :global(svg) {
    color: var(--warning-color);
  }
  .filters button:nth-child(3) :global(svg) {
    color: #a8b7c9;
  }
  .filters button:nth-child(4) :global(svg) {
    color: #6bbfc5;
  }
  .filters button:nth-child(5) :global(svg) {
    color: #8fc7de;
  }
  .filters button:nth-child(6) :global(svg) {
    color: #f5c842;
  }

  .filter-dropdowns {
    gap: 6px;
  }

  .dropdown-wrapper {
    position: relative;
  }

  .filter-dropdown-btn {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    height: 29px;
    width: 80px;
    padding: 0 8px;
    border: 1px solid transparent;
    border-radius: 6px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
    font-size: 11.5px;
    white-space: nowrap;
    transition:
      color 100ms ease,
      border-color 100ms ease,
      background 100ms ease;
  }

  .filter-dropdown-btn:hover {
    color: var(--text-secondary);
    border-color: var(--border-color);
    background: var(--hover-bg);
  }

  .dropdown-label {
    min-width: 0;
    max-width: 80px;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .dropdown-popover {
    position: absolute;
    top: calc(100% + 4px);
    right: 0;
    min-width: 100%;
    max-width: 150px;
    overflow: hidden;
  }

  .dropdown-backdrop {
    position: fixed;
    inset: 0;
    z-index: -1;
  }

  .dropdown-search {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--border-subtle);
    color: var(--text-faint);
  }

  .dropdown-search input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font-size: 12px;
  }

  .dropdown-search input::placeholder {
    color: var(--placeholder-color);
  }

  .dropdown-items {
    max-height: 180px;
    overflow-y: auto;
  }

  .toolbar-actions {
    flex: 0 0 auto;
    gap: 2px;
  }

  .toolbar-actions button {
    width: 28px;
    height: 28px;
    padding: 0;
    border-radius: 6px;
    color: var(--text-faint);
  }

  .toolbar-actions button:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }

  .toolbar-actions button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  /* Spans the route's split-detail grid: the grid belongs to the route, and
     it is the only cross-scope rule for this bar. */
  :global(.app-shell.split-detail) > .toolbar {
    grid-column: 1 / -1;
  }
  /* Narrow-window behavior lives with the component: page-scoped selectors
     cannot reach component internals. Breakpoint mirrors the route (660px). */
  @media (max-width: 660px) {
    .filter-dropdowns {
      display: none;
    }
    .toolbar-actions {
      display: none;
    }
  }
</style>
