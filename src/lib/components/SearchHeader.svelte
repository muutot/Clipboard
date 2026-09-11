<script lang="ts">
  import { assets } from "$app/paths";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { normalizeSearchTerm } from "$lib/utils/search-history";

  interface SearchOption {
    value: string;
  }

  interface Props {
    query: string;
    inputEl?: HTMLInputElement | null;
    placeholder: string;
    autocomplete: "none" | "inline" | "list" | "both";
    height: number;
    fontSize: number;
    showSuggestions: boolean;
    activeOptionIndex: number;
    historyOptions: SearchOption[];
    suggestionOptions: SearchOption[];
    inlineSuggestionSuffix: string | null;
    onfocus: () => void;
    oninput: () => void;
    onblur: () => void;
    onkeydown: (event: KeyboardEvent) => void;
    onchoose: (value: string) => void;
    onclear: () => void;
  }

  let {
    query = $bindable(),
    inputEl = $bindable(null),
    placeholder,
    autocomplete,
    height,
    fontSize,
    showSuggestions,
    activeOptionIndex,
    historyOptions,
    suggestionOptions,
    inlineSuggestionSuffix,
    onfocus,
    oninput,
    onblur,
    onkeydown,
    onchoose,
    onclear,
  }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);
</script>

<header
  class="search-header"
  role="presentation"
  aria-label={_t("actions.dragWindow")}
  onmousedown={(e) => {
    if (e.target === e.currentTarget) void getCurrentWindow().startDragging();
  }}
>
  <div class="search-box">
    <input
      bind:this={inputEl}
      bind:value={query}
      aria-label={placeholder}
      aria-autocomplete={autocomplete}
      aria-controls={showSuggestions ? "search-suggestions" : undefined}
      aria-expanded={showSuggestions}
      aria-activedescendant={activeOptionIndex >= 0
        ? `search-option-${activeOptionIndex}`
        : undefined}
      autocomplete="off"
      {placeholder}
      spellcheck="false"
      style={`height: ${height}px; font-size: ${fontSize}px;`}
      {onfocus}
      {oninput}
      {onblur}
      {onkeydown}
    />
    {#if inlineSuggestionSuffix !== null}
      <span class="search-inline-hint" aria-hidden="true" style={`font-size: ${fontSize}px;`}>
        <span>{normalizeSearchTerm(query)}</span>{inlineSuggestionSuffix}
      </span>
    {/if}
    {#if showSuggestions}
      <div
        id="search-suggestions"
        class="search-suggestions"
        role="listbox"
        aria-label={_t("search.suggestionsLabel")}
      >
        {#if historyOptions.length > 0}
          <div class="search-suggestions-heading">{_t("search.recent")}</div>
          {#each historyOptions as option, index (option.value)}
            <button
              id={`search-option-${index}`}
              type="button"
              role="option"
              tabindex="-1"
              aria-selected={activeOptionIndex === index}
              class:active={activeOptionIndex === index}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => onchoose(option.value)}
            >
              <AppIcon name="clock" size={14} />
              <span>{option.value}</span>
            </button>
          {/each}
        {/if}
        {#if suggestionOptions.length > 0}
          <div class="search-suggestions-heading">{_t("search.suggestions")}</div>
          {#each suggestionOptions as option, index (option.value)}
            {@const optionIndex = historyOptions.length + index}
            <button
              id={`search-option-${optionIndex}`}
              type="button"
              role="option"
              tabindex="-1"
              aria-selected={activeOptionIndex === optionIndex}
              class:active={activeOptionIndex === optionIndex}
              onmousedown={(event) => event.preventDefault()}
              onclick={() => onchoose(option.value)}
            >
              <AppIcon name="search" size={14} />
              <span>{option.value}</span>
            </button>
          {/each}
        {/if}
      </div>
    {/if}
    {#if query}
      <button
        class="clear-button"
        type="button"
        aria-label={_t("app.clearSearch")}
        onclick={onclear}>×</button
      >
    {/if}
  </div>
  <img
    class="brand-icon"
    src="{assets}/app-icon.png"
    alt="Clipboard"
    title="Clipboard"
    width="28"
    height="28"
  />
</header>

<style>
  /* Spans the route's split-detail grid: the grid belongs to the route, so
     this is the only cross-scope rule for this bar. */
  :global(.app-shell.split-detail) > .search-header {
    grid-column: 1 / -1;
  }

  .search-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 8px;
    border-bottom: none;
  }

  .search-box {
    position: relative;
    display: flex;
    flex: 1;
    align-items: center;
    gap: 10px;
    min-width: 0;
    color: var(--text-muted);
  }

  .search-suggestions {
    position: absolute;
    z-index: 110;
    top: calc(100% + 8px);
    left: 0;
    right: 0;
    max-height: min(280px, calc(100vh - 100px));
    padding: 6px 0;
    overflow-y: auto;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--surface-bg);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.48);
  }

  .search-suggestions-heading {
    padding: 5px 12px 3px;
    color: var(--text-faint);
    font-size: 10px;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }

  .search-suggestions button {
    display: flex;
    width: 100%;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 6px 12px;
    border: 0;
    color: var(--text-secondary);
    background: transparent;
    text-align: left;
    cursor: pointer;
    font-size: 12px;
  }

  .search-suggestions button :global(svg) {
    flex: 0 0 auto;
    color: var(--text-muted);
  }

  .search-suggestions button span {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .search-suggestions button:hover,
  .search-suggestions button.active {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .search-suggestions button.active :global(svg) {
    color: var(--selection-color);
  }

  .search-box input {
    flex: 1;
    min-width: 0;
    padding: 2px 0 0px;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font-size: clamp(17px, 3vw, 21px);
    font-weight: 350;
    letter-spacing: -0.02em;
  }

  .search-box input::placeholder {
    color: var(--placeholder-color);
    opacity: 1;
  }

  .search-inline-hint {
    position: absolute;
    top: 50%;
    left: 0;
    z-index: 0;
    overflow: hidden;
    max-width: calc(100% - 42px);
    color: var(--text-faint);
    pointer-events: none;
    transform: translateY(-50%);
    white-space: pre;
    font-size: clamp(17px, 3vw, 21px);
    font-weight: 350;
    letter-spacing: -0.02em;
  }

  .search-inline-hint span {
    visibility: hidden;
  }

  .clear-button {
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: var(--text-muted);
    background: var(--hover-bg);
    cursor: pointer;
  }

  .brand-icon {
    width: 28px;
    height: 28px;
    border-radius: 8px;
    object-fit: contain;
  }
</style>
