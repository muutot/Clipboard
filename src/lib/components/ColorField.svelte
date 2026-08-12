<script lang="ts">
  interface Props {
    value: string;
    onchange: (value: string) => void;
    disabled?: boolean;
    compact?: boolean;
    colorAriaLabel?: string;
    hexAriaLabel?: string;
  }

  let {
    value,
    onchange,
    disabled = false,
    compact = false,
    colorAriaLabel,
    hexAriaLabel,
  }: Props = $props();
</script>

<input
  type="color"
  class="color-picker"
  class:compact
  {value}
  {disabled}
  aria-label={colorAriaLabel}
  oninput={(e) => onchange((e.currentTarget as HTMLInputElement).value)}
/>
<input
  type="text"
  class="color-text-input"
  class:compact
  {value}
  {disabled}
  maxlength={9}
  aria-label={hexAriaLabel}
  oninput={(e) => {
    const val = (e.currentTarget as HTMLInputElement).value;
    if (/^#[0-9a-fA-F]{0,8}$/.test(val)) {
      onchange(val);
    }
  }}
/>

<style>
  .color-picker {
    width: 32px;
    height: 32px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    cursor: pointer;
    padding: 2px;
    background: var(--input-bg);
  }

  .color-picker.compact {
    width: 28px;
    height: 28px;
    flex-shrink: 0;
  }

  .color-picker:disabled {
    opacity: 0.5;
    cursor: default;
  }

  .color-text-input {
    width: 80px;
    padding: 5px 8px;
    background: var(--input-bg);
    color: var(--text-primary);
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    font-family: monospace;
    text-transform: uppercase;
  }

  .color-text-input.compact {
    width: 74px;
    flex-shrink: 0;
    padding: 4px 6px;
  }

  .color-text-input:focus {
    border-color: var(--text-faint);
    outline: none;
  }

  .color-text-input:disabled {
    opacity: 0.5;
    cursor: default;
  }
</style>
