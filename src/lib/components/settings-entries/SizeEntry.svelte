<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import type { SizeEntryConfig, SizeUnit } from "$lib/types/settings-entry";
  import { SIZE_UNIT_OPTIONS } from "$lib/utils/size";

  interface Props {
    config: SizeEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();

  function handleNumberInput(event: Event, set: (v: number) => void) {
    const raw = (event.currentTarget as HTMLInputElement).value;
    if (raw === "") return;
    const value = Number(raw);
    if (Number.isNaN(value)) return;
    set(value);
  }
</script>

<section
  class="setting-card setting-card-row"
  data-settings-search-id={searchId ?? config.id ?? undefined}
>
  {#if config.icon}
    <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
  {/if}
  <span class="setting-label">{config.label}</span>
  <input
    type="number"
    value={config.get()}
    min={config.min}
    aria-label={config.label}
    oninput={(e) => {
      handleNumberInput(e, config.set);
      config.oninput?.();
    }}
    onchange={config.onchange}
  />
  <CustomSelect
    className="unit-select"
    value={config.getUnit()}
    options={SIZE_UNIT_OPTIONS}
    ariaLabel={config.label}
    onchange={(v) => config.setUnit(v as SizeUnit)}
  />
</section>
