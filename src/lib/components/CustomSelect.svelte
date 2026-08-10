<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { alignDropdownOptionText, resolveFixedPopoverPosition } from "$lib/utils/dropdown";

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
  let popoverTop = $state(0);
  let popoverLeft = $state(0);
  let popoverWidth = $state(0);
  let triggerEl: HTMLButtonElement | undefined = $state();
  let popoverEl: HTMLDivElement | undefined = $state();

  const current = $derived(
    options.find((o) => o.value === value)?.label ?? value?.toString() ?? placeholder,
  );

  function positionPopover() {
    if (!open || !triggerEl || !popoverEl) return;
    const rect = triggerEl.getBoundingClientRect();
    const targetWidth = Math.max(rect.width, popoverEl.offsetWidth);
    popoverWidth = Math.min(targetWidth, 150);
    const position = resolveFixedPopoverPosition(rect, popoverWidth, popoverEl.offsetHeight, {
      align: "end",
    });
    popoverTop = position.top;
    popoverLeft = position.left;
  }

  function toggle() {
    if (disabled) return;
    open = !open;
  }

  $effect(() => {
    if (!open) return;
    positionPopover();
    if (popoverEl) alignDropdownOptionText(popoverEl);
    window.addEventListener("resize", positionPopover);
    window.addEventListener("scroll", onScroll, true);
    return () => {
      window.removeEventListener("resize", positionPopover);
      window.removeEventListener("scroll", onScroll, true);
    };
  });

  function onScroll(e: Event) {
    if (popoverEl && e.target instanceof Node && popoverEl.contains(e.target)) return;
    open = false;
  }

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
    bind:this={triggerEl}
    onclick={toggle}
    onkeydown={(e) => {
      if (e.key === "Escape") open = false;
    }}
  >
    <span class="custom-select-value">{current}</span>
    <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
  </button>

  {#if open}
    <div
      class="custom-select-popover popover-surface"
      role="listbox"
      aria-label={ariaLabel}
      style="top: {popoverTop}px; left: {popoverLeft}px; min-width: {popoverWidth}px;"
      bind:this={popoverEl}
    >
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
          <span>{option.label}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>
