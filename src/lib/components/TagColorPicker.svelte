<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import {
    isTagColorPreset,
    isValidTagColor,
    TAG_COLOR_FALLBACK,
    TAG_COLOR_PRESETS,
  } from "$lib/data/tag-colors";

  interface Props {
    value: string;
    customLabel: string;
    onchange: (value: string) => void;
    ariaLabel?: string;
    size?: number;
    gap?: number;
  }

  let { value, customLabel, onchange, ariaLabel, size = 22, gap = 8 }: Props = $props();

  const customActive = $derived(value !== "" && !isTagColorPreset(value));
  const customInputValue = $derived(isValidTagColor(value) ? value : TAG_COLOR_FALLBACK);

  function selectPreset(color: string): void {
    onchange(value === color ? "" : color);
  }
</script>

<div
  class="tag-color-picker"
  role={ariaLabel ? "group" : undefined}
  aria-label={ariaLabel}
  style={`--tag-color-option-size: ${size}px; --tag-color-option-gap: ${gap}px;`}
>
  {#each TAG_COLOR_PRESETS as color (color)}
    <button
      type="button"
      class="tag-swatch-option"
      class:active={value === color}
      style={`--swatch: ${color}`}
      aria-label={color}
      title={color}
      onclick={() => selectPreset(color)}
    ></button>
  {/each}
  <label
    class="tag-swatch-option tag-swatch-custom"
    class:active={customActive}
    style={customActive ? `--swatch: ${value}` : undefined}
    title={customLabel}
    aria-label={customLabel}
  >
    <input
      type="color"
      value={customInputValue}
      onchange={(event) => onchange(event.currentTarget.value)}
    />
    <AppIcon name="palette" size={12} />
  </label>
</div>

<style>
  .tag-color-picker {
    display: grid;
    grid-template-columns: repeat(8, 1fr);
    align-items: center;
    justify-items: center;
    gap: var(--tag-color-option-gap);
  }

  .tag-swatch-option {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--tag-color-option-size);
    height: var(--tag-color-option-size);
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 50%;
    background: var(--surface-bg);
    cursor: pointer;
  }

  .tag-swatch-option[style*="--swatch"] {
    background: var(--swatch);
  }

  .tag-swatch-option.active {
    outline: 2px solid var(--text-primary);
    outline-offset: 1px;
  }

  .tag-swatch-custom {
    position: relative;
    overflow: hidden;
    color: var(--text-secondary);
    background: conic-gradient(
      from 180deg,
      #e5484d,
      #f76b15,
      #ffb224,
      #46a758,
      #3e63dd,
      #8e4ec6,
      #00a2c7,
      #e5484d
    );
  }

  .tag-swatch-custom input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    padding: 0;
    border: 0;
    opacity: 0;
    cursor: pointer;
  }

  .tag-swatch-custom.active {
    color: var(--text-primary);
  }
</style>
