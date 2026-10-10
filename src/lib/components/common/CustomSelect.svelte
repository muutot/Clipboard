<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";
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
    /** Shown while `value` is empty; has no effect once an option matches. */
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

  // Label precedence: the matching option, then the placeholder while nothing is
  // selected, then the raw value. The empty-string check is explicit because
  // `??` would short-circuit on `String("")` and make `placeholder` unreachable.
  const current = $derived(
    options.find((o) => o.value === value)?.label ??
      (value === "" ? placeholder : String(value ?? "")),
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

  function close(restoreFocus = false) {
    open = false;
    if (restoreFocus) triggerEl?.focus();
  }

  function enabledOptions() {
    return [
      ...(popoverEl?.querySelectorAll<HTMLButtonElement>('[role="option"]:not(:disabled)') ?? []),
    ];
  }

  $effect(() => {
    if (!open || !popoverEl) return;
    const choices = enabledOptions();
    (
      choices.find((option) => option.getAttribute("aria-selected") === "true") ??
      choices[0] ??
      popoverEl
    ).focus();
  });

  function navigateOptions(event: KeyboardEvent) {
    if (event.key === "Tab") {
      // Let the browser continue tab order from the stable trigger after the
      // focused option is removed. Shift+Tab naturally goes to the prior control.
      close(true);
      return;
    }
    const choices = enabledOptions();
    if (!choices.length || !["ArrowDown", "ArrowUp", "Home", "End"].includes(event.key)) return;
    event.preventDefault();
    event.stopPropagation();
    const current = choices.findIndex((option) => option === document.activeElement);
    const next =
      event.key === "Home"
        ? 0
        : event.key === "End"
          ? choices.length - 1
          : (current + (event.key === "ArrowDown" ? 1 : -1) + choices.length) % choices.length;
    choices[next].focus();
  }

  $effect(() => {
    if (!open) return;
    positionPopover();
    if (popoverEl) alignDropdownOptionText(popoverEl);
    window.addEventListener("resize", positionPopover);
    window.addEventListener("scroll", onScroll, true);
    const onEscapeCapture = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.preventDefault();
      e.stopImmediatePropagation();
      close(true);
    };
    window.addEventListener("keydown", onEscapeCapture, true);
    return () => {
      window.removeEventListener("resize", positionPopover);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("keydown", onEscapeCapture, true);
    };
  });

  function onScroll(e: Event) {
    if (popoverEl && e.target instanceof Node && popoverEl.contains(e.target)) return;
    close(!!popoverEl?.contains(document.activeElement));
  }

  function select(option: CustomSelectOption) {
    if (option.disabled) return;
    close(true);
    if (value === option.value) return;
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
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (!disabled) open = true;
        e.preventDefault();
        e.stopPropagation();
      } else if (e.key === "Escape" && open) {
        close(true);
        e.preventDefault();
        e.stopPropagation();
      }
    }}
  >
    <span class="custom-select-value">{current}</span>
    <AppIcon name="chevron-down" size={12} strokeWidth={2.5} />
  </button>

  {#if open}
    <div
      class="custom-select-popover popover-surface"
      role="listbox"
      tabindex="-1"
      aria-label={ariaLabel}
      style="top: {popoverTop}px; left: {popoverLeft}px; min-width: {popoverWidth}px;"
      bind:this={popoverEl}
      onkeydown={navigateOptions}
    >
      <div class="custom-select-backdrop" onclick={() => close(true)} aria-hidden="true"></div>
      {#each options as option}
        <button
          type="button"
          role="option"
          tabindex="-1"
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
