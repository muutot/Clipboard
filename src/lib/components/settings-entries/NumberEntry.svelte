<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { NumberEntryConfig } from "$lib/types/settings-entry";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: NumberEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();

  let isRow = $derived((config.variant ?? "row") === "row");

  function handleNumberInput(event: Event, set: (v: number) => void) {
    const raw = (event.currentTarget as HTMLInputElement).value;
    if (raw === "") return;
    const value = Number(raw);
    if (Number.isNaN(value)) return;
    set(value);
  }
</script>

{#if !isRow}
  <section class="setting-card" data-settings-search-id={searchId ?? config.id ?? undefined}>
    <div class="setting-heading">
      {#snippet children()}
        {#if config.suffix}<span class="value-label">{config.get()}{config.suffix}</span>{/if}
      {/snippet}
      <EntryHeading
        icon={config.icon}
        label={config.label}
        desc={config.desc}
        inline={!!config.suffix || !!config.desc}
        {children}
      />
    </div>
    <input
      type="number"
      value={config.get()}
      min={config.min}
      max={config.max}
      step={config.step}
      aria-label={config.label}
      oninput={(e) => handleNumberInput(e, config.set)}
      onchange={config.onchange}
      onblur={config.onblur}
    />
  </section>
{:else}
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
      max={config.max}
      step={config.step}
      aria-label={config.label}
      oninput={(e) => handleNumberInput(e, config.set)}
      onchange={config.onchange}
      onblur={config.onblur}
    />
    {#if config.suffix}<span class="number-suffix">{config.suffix}</span>{/if}
  </section>
{/if}
