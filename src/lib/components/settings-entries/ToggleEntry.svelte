<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { ToggleEntryConfig } from "$lib/types/settings-entry";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: ToggleEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();

  let isRow = $derived((config.variant ?? "card") === "row");
</script>

<section
  class="setting-card {isRow ? 'setting-card-row' : 'toggle-card'}"
  data-settings-search-id={searchId ?? config.id ?? undefined}
>
  {#if !isRow}
    <div class="setting-heading">
      <EntryHeading icon={config.icon} label={config.label} desc={config.desc} />
    </div>
  {:else}
    {#if config.icon}
      <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
    {/if}
    <span class="setting-label">{config.label}</span>
  {/if}
  <button
    type="button"
    class="toggle-switch"
    class:active={config.get()}
    onclick={() => {
      config.set(!config.get());
      config.onchange?.();
    }}
    aria-checked={config.get()}
    aria-label={config.label}
    role="switch"
    disabled={config.disabled?.()}
  >
    <span class="toggle-knob"></span>
  </button>
</section>
