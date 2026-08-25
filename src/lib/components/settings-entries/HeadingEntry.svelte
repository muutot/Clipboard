<script lang="ts">
  import type { HeadingEntryConfig } from "$lib/types/settings-entry";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: HeadingEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();
</script>

<section class="setting-card" data-settings-search-id={searchId ?? config.id ?? undefined}>
  <div class="setting-heading">
    <EntryHeading icon={config.icon} label={config.label} desc={config.desc} />
    {#if config.actionLabel}
      <button
        type="button"
        class="settings-action-btn"
        disabled={config.actionDisabled}
        onclick={config.onaction}
      >
        {config.actionLabel}
      </button>
    {/if}
  </div>
</section>
