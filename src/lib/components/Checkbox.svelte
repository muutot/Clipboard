<script lang="ts">
  import AppIcon from "./AppIcon.svelte";

  interface Props {
    checked: boolean;
    onchange?: (checked: boolean) => void;
    disabled?: boolean;
    size?: number;
    ariaLabel?: string;
    onclick?: (event: MouseEvent) => void;
  }

  let { checked, onchange, disabled = false, size = 15, ariaLabel = "", onclick }: Props = $props();
</script>

<input
  type="checkbox"
  {checked}
  {disabled}
  aria-label={ariaLabel || undefined}
  onchange={(e) => onchange?.((e.target as HTMLInputElement).checked)}
  {onclick}
/>
<span class="check-mark" style:--checkbox-size={`${size}px`}>
  <AppIcon name="check" size={Math.max(10, Math.round(size * 0.75))} strokeWidth={2.5} mono />
</span>

<style>
  input {
    position: absolute;
    opacity: 0;
    width: 0;
    height: 0;
    margin: 0;
  }

  .check-mark {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: var(--checkbox-size, 15px);
    height: var(--checkbox-size, 15px);
    flex-shrink: 0;
    border: 1.5px solid var(--text-faint);
    border-radius: 4px;
    color: transparent;
    background: transparent;
    transition:
      background 100ms ease,
      border-color 100ms ease,
      color 100ms ease;
  }

  input:checked + .check-mark {
    border-color: var(--selection-color);
    background: var(--selection-color);
    color: #fff;
  }

  input:disabled + .check-mark {
    opacity: 0.5;
  }

  input:focus-visible + .check-mark {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
