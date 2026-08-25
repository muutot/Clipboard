<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { CustomEntryConfig } from "$lib/types/settings-entry";
  import type { Snippet } from "svelte";

  interface Props {
    config: CustomEntryConfig;
    children?: Snippet;
    searchId?: string;
  }

  let { config, children, searchId }: Props = $props();

  function resolveActionLabel(label: string | (() => string)): string {
    return typeof label === "function" ? label() : label;
  }

  let isToggle = $derived((config.variant ?? "toggle") === "toggle");
</script>

{#if isToggle}
  <section
    class="setting-card toggle-card"
    data-settings-search-id={searchId ?? config.id ?? undefined}
  >
    <div class="setting-heading">
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <div>
        <strong>{config.label}</strong>
        {#if config.desc}<p>{config.desc}</p>{/if}
      </div>
    </div>
    {#if children}{@render children()}{/if}
  </section>
{:else}
  <section
    class="setting-card setting-card-custom-column"
    data-settings-search-id={searchId ?? config.id ?? undefined}
  >
    <div class="setting-heading">
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <div class="heading-inline">
        <div>
          <strong>{config.label}</strong>
          {#if config.desc}<p>{config.desc}</p>{/if}
        </div>
        {#if config.actionLabel && (!config.actionVisible || config.actionVisible())}
          <button type="button" class="settings-action-btn" onclick={config.onaction}
            >{resolveActionLabel(config.actionLabel)}</button
          >
        {/if}
      </div>
    </div>
    {#if children}{@render children()}{/if}
  </section>
{/if}
