<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";

  interface Props {
    value: string;
    oninput: (value: string) => void;
    placeholder?: string;
    ariaLabel?: string;
    width?: number;
    margin?: string;
  }

  let { value, oninput, placeholder, ariaLabel, width, margin }: Props = $props();
</script>

<label class="search-field" style:width={width ? `${width}px` : undefined} style:margin>
  <AppIcon name="search" size={15} />
  <input
    type="search"
    {value}
    {placeholder}
    aria-label={ariaLabel}
    oninput={(e) => oninput((e.currentTarget as HTMLInputElement).value)}
  />
</label>

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
</style>
