<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import type { TextEntryConfig } from "$lib/types/settings-entry";
  import EntryHeading from "./EntryHeading.svelte";

  interface Props {
    config: TextEntryConfig;
    searchId?: string;
  }

  let { config, searchId }: Props = $props();

  let isRow = $derived((config.variant ?? "card") === "row");

  function resolveActionLabel(label: string | (() => string)): string {
    return typeof label === "function" ? label() : label;
  }
</script>

{#if !isRow}
  <section class="setting-card" data-settings-search-id={searchId ?? config.id ?? undefined}>
    <div class="setting-heading">
      {#snippet children()}
        {#if config.actionLabel && (!config.actionVisible || config.actionVisible())}
          <button type="button" class="settings-action-btn" onclick={config.onaction}
            >{resolveActionLabel(config.actionLabel)}</button
          >
        {/if}
      {/snippet}
      <EntryHeading icon={config.icon} label={config.label} desc={config.desc} inline {children} />
    </div>
    <input
      type={config.inputType ?? "text"}
      class="settings-text-input"
      value={config.get()}
      maxlength={config.maxlength}
      placeholder={config.placeholder}
      aria-label={config.label}
      oninput={(e) => config.set((e.currentTarget as HTMLInputElement).value)}
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
      type={config.inputType ?? "text"}
      value={config.get()}
      maxlength={config.maxlength}
      placeholder={config.placeholder}
      aria-label={config.label}
      style="flex:1;min-width:0"
      oninput={(e) => config.set((e.currentTarget as HTMLInputElement).value)}
      onchange={config.onchange}
      onblur={config.onblur}
    />
  </section>
{/if}

<style>
  .settings-text-input {
    width: 100%;
    margin-top: 12px;
    padding: 7px 10px;
    border: 1px solid var(--border-color);
    border-radius: var(--settings-control-radius, 6px);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: var(--settings-control-size, var(--font-size-secondary, 11px));
    outline: none;
    transition: border-color 120ms ease;
  }

  .settings-text-input::placeholder {
    color: var(--placeholder-color);
  }

  .settings-text-input:focus {
    border-color: var(--text-faint);
  }
</style>
