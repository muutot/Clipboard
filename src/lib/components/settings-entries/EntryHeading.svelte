<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { IconName } from "$lib/types/clipboard";
  import type { Snippet } from "svelte";

  interface Props {
    icon?: IconName;
    label: string;
    desc?: string;
    /** Trailing element (value label / action button); forces the inline layout. */
    children?: Snippet;
    inline?: boolean;
  }

  let { icon, label, desc, children, inline = false }: Props = $props();
</script>

{#if icon}
  <span class="setting-icon"><AppIcon name={icon} size={17} /></span>
{/if}
{#if children || inline}
  <div class="heading-inline">
    <div>
      <strong>{label}</strong>
      {#if desc}<p>{desc}</p>{/if}
    </div>
    {@render children?.()}
  </div>
{:else}
  <div>
    <strong>{label}</strong>
    {#if desc}<p>{desc}</p>{/if}
  </div>
{/if}
