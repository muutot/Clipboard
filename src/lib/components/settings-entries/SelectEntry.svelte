<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import type { SelectEntryConfig } from "$lib/types/settings-entry";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: SelectEntryConfig;
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
  <CustomSelect
    value={config.get()}
    options={config.options}
    ariaLabel={config.ariaLabel ?? config.label}
    disabled={config.disabled}
    onchange={(v) => {
      config.set(v);
      config.onchange?.();
    }}
  />
</section>
