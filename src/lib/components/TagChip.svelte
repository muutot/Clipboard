<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";

  interface Props {
    tag: string;
    accent?: string;
    onremove?: (tag: string) => void;
    hoverReveal?: boolean;
    compact?: boolean;
    removeAriaLabel?: string;
    onclick?: (tag: string) => void;
    oncontextmenu?: (tag: string) => void;
  }

  let {
    tag,
    accent,
    onremove,
    hoverReveal = false,
    compact = false,
    removeAriaLabel,
    onclick,
    oncontextmenu,
  }: Props = $props();
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<span
  class="tag-chip"
  class:compact
  class:hover-reveal={hoverReveal}
  role={onclick ? "button" : undefined}
  tabindex={onclick ? 0 : undefined}
  title={onclick ? tag : undefined}
  style={accent ? `--tag-accent: ${accent}` : undefined}
  onclick={(e) => {
    if (!onclick) return;
    e.stopPropagation();
    onclick(tag);
  }}
  onkeydown={(e) => {
    if (onclick && e.key === "Enter") {
      e.stopPropagation();
      onclick(tag);
    }
  }}
  oncontextmenu={(e) => {
    if (!oncontextmenu) return;
    e.preventDefault();
    e.stopPropagation();
    oncontextmenu(tag);
  }}
>
  {tag}
  {#if onremove}
    <button
      type="button"
      class="tag-chip-remove"
      aria-label={removeAriaLabel}
      onclick={(e) => {
        e.stopPropagation();
        onremove(tag);
      }}
    >
      <AppIcon name="x" size={compact ? 10 : 11} />
    </button>
  {/if}
</span>

<style>
  .tag-chip {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 2px 6px;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-secondary);
    background: var(--surface-bg);
    font-size: 11px;
    line-height: 1.4;
    white-space: nowrap;
    user-select: none;
  }

  .tag-chip.compact {
    padding: 1px 5px;
    font-size: 10.5px;
    line-height: 1.5;
    cursor: pointer;
  }

  .tag-chip:hover {
    color: var(--text-primary);
    border-color: var(--text-faint);
  }

  .tag-chip[style*="--tag-accent"] {
    border-color: color-mix(in srgb, var(--tag-accent) 55%, transparent);
    color: var(--tag-accent);
    background: color-mix(in srgb, var(--tag-accent) 12%, var(--surface-bg));
  }

  .tag-chip[style*="--tag-accent"]:hover {
    color: var(--tag-accent);
    border-color: var(--tag-accent);
  }

  .tag-chip-remove {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 14px;
    height: 14px;
    padding: 0;
    border: 0;
    border-radius: 3px;
    color: var(--text-faint);
    background: transparent;
    cursor: pointer;
  }

  .tag-chip-remove:hover {
    color: var(--danger-color);
    background: var(--hover-bg);
  }

  .tag-chip.hover-reveal .tag-chip-remove {
    position: absolute;
    top: -3px;
    right: -3px;
    z-index: 2;
    width: 13px;
    height: 13px;
    border-radius: 50%;
    background: var(--surface-bg);
    box-shadow: 0 0 0 1px var(--border-color);
    opacity: 0;
    pointer-events: none;
    transition: opacity 100ms ease;
  }

  .tag-chip.hover-reveal:hover .tag-chip-remove,
  .tag-chip.hover-reveal .tag-chip-remove:focus-visible {
    opacity: 1;
    pointer-events: auto;
  }

  .tag-chip.hover-reveal .tag-chip-remove:hover {
    color: var(--danger-color);
    border-color: var(--danger-color);
    background: var(--surface-bg);
  }
</style>
