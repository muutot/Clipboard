<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import type { IconName } from "$lib/types/clipboard";
  import type { Snippet } from "svelte";

  interface Props {
    icon?: IconName;
    /** Icon glyph size; the standard heading uses 17, theme presets use 16. */
    iconSize?: number;
    label: string;
    desc?: string;
    /** Class for the description `<p>` (panel-specific styling). */
    descClass?: string;
    /** Class for the inner text `<div>` holding the label/description. */
    textClass?: string;
    /** Extra class merged onto the `.setting-heading` wrapper. */
    class?: string;
    /** Trailing element (value label / action button); forces the inline layout. */
    children?: Snippet;
    /** Element rendered as a direct flex child of `.setting-heading` after the text (not inside `.heading-inline`). */
    trailing?: Snippet;
    inline?: boolean;
  }

  let {
    icon,
    iconSize = 17,
    label,
    desc,
    descClass,
    textClass,
    class: className,
    children,
    trailing,
    inline = false,
  }: Props = $props();

  let wrapperClass = $derived(className ? `setting-heading ${className}` : "setting-heading");
</script>

<div class={wrapperClass}>
  {#if icon}
    <span class="setting-icon"><AppIcon name={icon} size={iconSize} /></span>
  {/if}
  {#if children || inline}
    <div class="heading-inline">
      <div class={textClass}>
        <strong>{label}</strong>
        {#if desc}<p class={descClass}>{desc}</p>{/if}
      </div>
      {@render children?.()}
    </div>
  {:else}
    <div class={textClass}>
      <strong>{label}</strong>
      {#if desc}<p class={descClass}>{desc}</p>{/if}
    </div>
  {/if}
  {@render trailing?.()}
</div>
