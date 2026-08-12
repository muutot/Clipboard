<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";

  interface Props {
    value: string;
    oninput: (value: string) => void;
    placeholder?: string;
    ariaLabel?: string;
    width?: number;
    margin?: string;
    clearLabel?: string;
    onclear?: () => void;
    fill?: boolean;
    sidebar?: boolean;
    labelFor?: string;
    id?: string;
    autocomplete?: "off" | "on";
    spellcheck?: boolean;
  }

  let {
    value,
    oninput,
    placeholder,
    ariaLabel,
    width,
    margin,
    clearLabel,
    onclear,
    fill = false,
    sidebar = false,
    labelFor,
    id,
    autocomplete,
    spellcheck,
  }: Props = $props();
</script>

<div
  class="search-field"
  class:fill
  class:sidebar
  style:width={width ? `${width}px` : undefined}
  style:margin
>
  <AppIcon name="search" size={15} />
  {#if labelFor}
    <label class="visually-hidden" for={labelFor}>{ariaLabel}</label>
  {/if}
  <input
    {id}
    type="search"
    {value}
    {placeholder}
    aria-label={ariaLabel}
    {autocomplete}
    {spellcheck}
    oninput={(e) => oninput((e.currentTarget as HTMLInputElement).value)}
  />
  {#if onclear && value}
    <button type="button" class="search-clear" aria-label={clearLabel} onclick={onclear}>×</button>
  {/if}
</div>

<style>
  .search-field {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 7px 9px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    color: var(--text-faint);
    background: var(--input-bg);
  }

  .search-field.fill {
    flex: 1;
  }

  .search-field.sidebar {
    gap: 8px;
    padding: 0 9px;
    color: var(--text-muted);
    transition:
      border-color 100ms ease,
      background 100ms ease;
  }

  .search-field.sidebar:focus-within {
    border-color: var(--text-faint);
    background: var(--hover-bg);
  }

  .search-field.sidebar input {
    width: 100%;
    padding: 7px 0;
  }

  .search-field.sidebar input::placeholder {
    color: var(--placeholder-color);
  }

  .search-field input {
    min-width: 0;
    flex: 1;
    border: 0;
    outline: 0;
    color: var(--text-primary);
    background: transparent;
    font: inherit;
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
  }

  .search-field input::-webkit-search-cancel-button {
    display: none;
  }

  .search-clear {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 20px;
    height: 20px;
    flex: 0 0 auto;
    padding: 0;
    border: 0;
    border-radius: 50%;
    color: var(--text-muted);
    background: transparent;
    font-size: 16px;
    line-height: 1;
    cursor: pointer;
  }

  .search-clear:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }
</style>
