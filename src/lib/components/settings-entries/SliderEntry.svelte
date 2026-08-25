<script lang="ts">
  import type { SliderEntryConfig } from "$lib/types/settings-entry";
  import { sliderPercentage } from "$lib/utils/format";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: SliderEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();

  function resolveBound(value: number | (() => number)): number {
    return typeof value === "function" ? value() : value;
  }
</script>

<section class="setting-card" data-settings-search-id={searchId ?? config.id ?? undefined}>
  <div class="setting-heading">
    {#snippet children()}
      <span class="value-label">{config.get()}{config.suffix}</span>
    {/snippet}
    <EntryHeading icon={config.icon} label={config.label} desc={config.desc} inline {children} />
  </div>
  <input
    type="range"
    class="transparency-slider"
    min={resolveBound(config.min)}
    max={resolveBound(config.max)}
    step={config.step}
    value={config.get()}
    style:--slider-pct={sliderPercentage(
      config.get(),
      resolveBound(config.min),
      resolveBound(config.max),
    )}
    aria-label={config.label}
    oninput={(e) => {
      config.set(Number((e.currentTarget as HTMLInputElement).value));
      config.oninput?.();
    }}
  />
</section>
