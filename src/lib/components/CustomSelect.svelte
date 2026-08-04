<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";

  export interface CustomSelectOption {
    value: string | number;
    label: string;
    disabled?: boolean;
  }

  interface Props {
    value: string | number;
    options: CustomSelectOption[];
    onchange: (value: string | number) => void;
    className?: string;
    ariaLabel?: string;
    title?: string;
    disabled?: boolean;
    placeholder?: string;
  }

  let {
    value,
    options,
    onchange,
    className = "",
    ariaLabel,
    title,
    disabled = false,
    placeholder = "",
  }: Props = $props();

  let open = $state(false);

  const current = $derived(
    options.find((o) => o.value === value)?.label ?? value?.toString() ?? placeholder,
  );

  function select(option: CustomSelectOption) {
    if (option.disabled || value === option.value) {
      open = false;
      return;
    }
    open = false;
    onchange(option.value);
  }
</script>

<div class="custom-select {className}">
  <button
    type="button"
    class="settings-select"
    aria-haspopup="listbox"
    aria-expanded={open}
    aria-label={ariaLabel}
    {title}
    {disabled}
    onclick={() => (open = !open)}
    onkeydown={(e) => {
      if (e.key === "Escape") open = false;
    }}
  >
    <span class="custom-select-value">{current}</span>
    <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
  </button>

  {#if open}
    <div class="custom-select-popover" role="listbox" aria-label={ariaLabel}>
      <div class="custom-select-backdrop" onclick={() => (open = false)} aria-hidden="true"></div>
      {#each options as option}
        <button
          type="button"
          role="option"
          aria-selected={option.value === value}
          class:selected={option.value === value}
          disabled={option.disabled}
          onclick={() => select(option)}
        >
          {option.label}
        </button>
      {/each}
    </div>
  {/if}
</div>
