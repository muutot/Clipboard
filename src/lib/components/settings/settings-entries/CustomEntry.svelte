<script lang="ts">
  import type { CustomEntryConfig } from "$lib/types/settings-entry";
  import type { Snippet } from "svelte";
  import EntryHeading from "./EntryHeading.svelte";

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
    <EntryHeading icon={config.icon} label={config.label} desc={config.desc} />
    {#if children}{@render children()}{/if}
  </section>
{:else}
  <section
    class="setting-card setting-card-custom-column"
    data-settings-search-id={searchId ?? config.id ?? undefined}
  >
    {#snippet headingAction()}
      {#if config.actionLabel && (!config.actionVisible || config.actionVisible())}
        <button type="button" class="settings-action-btn" onclick={config.onaction}
          >{resolveActionLabel(config.actionLabel)}</button
        >
      {/if}
    {/snippet}
    <EntryHeading
      icon={config.icon}
      label={config.label}
      desc={config.desc}
      inline
      children={headingAction}
    />
    {#if children}{@render children()}{/if}
  </section>
{/if}
