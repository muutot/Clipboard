<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import CustomSelect from "$lib/components/CustomSelect.svelte";
  import type { SettingEntryConfig, SizeUnit } from "$lib/types/settings-entry";
  import { sliderPercentage } from "$lib/utils/format";
  import { SIZE_UNIT_OPTIONS } from "$lib/utils/size";
  import type { Snippet } from "svelte";

  interface Props {
    config: SettingEntryConfig;
    children?: Snippet;
  }

  let { config, children }: Props = $props();

  function resolveBound(value: number | (() => number)): number {
    return typeof value === "function" ? value() : value;
  }

  function handleNumberInput(event: Event, set: (v: number) => void) {
    const el = event.currentTarget as HTMLInputElement;
    const raw = el.value;
    if (raw === "") return;
    const value = Number(raw);
    if (Number.isNaN(value)) return;
    set(value);
  }

  function handleRangeInput(event: Event, set: (v: number) => void) {
    set(Number((event.currentTarget as HTMLInputElement).value));
  }

  function resolveActionLabel(label: string | (() => string)): string {
    return typeof label === "function" ? label() : label;
  }
</script>

{#if config.type === "heading"}
  <section class="setting-card" data-settings-search-id={config.id ?? undefined}>
    <div class="setting-heading">
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <div>
        <strong>{config.label}</strong>
        {#if config.desc}<p>{config.desc}</p>{/if}
      </div>
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
{:else if config.type === "slider"}
  <section class="setting-card" data-settings-search-id={config.id ?? undefined}>
    <div class="setting-heading">
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <div class="heading-inline">
        <div>
          <strong>{config.label}</strong>
          {#if config.desc}<p>{config.desc}</p>{/if}
        </div>
        <span class="value-label">{config.get()}{config.suffix}</span>
      </div>
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
        handleRangeInput(e, config.set);
        config.oninput?.();
      }}
    />
  </section>
{:else if config.type === "toggle"}
  {#if (config.variant ?? "card") === "card"}
    <section class="setting-card toggle-card" data-settings-search-id={config.id ?? undefined}>
      <div class="setting-heading">
        {#if config.icon}
          <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
        {/if}
        <div>
          <strong>{config.label}</strong>
          {#if config.desc}<p>{config.desc}</p>{/if}
        </div>
      </div>
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
  {:else}
    <section class="setting-card setting-card-row" data-settings-search-id={config.id ?? undefined}>
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <span class="setting-label">{config.label}</span>
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
  {/if}
{:else if config.type === "select"}
  {#if (config.variant ?? "card") === "card"}
    <section class="setting-card toggle-card" data-settings-search-id={config.id ?? undefined}>
      <div class="setting-heading">
        {#if config.icon}
          <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
        {/if}
        <div>
          <strong>{config.label}</strong>
          {#if config.desc}<p>{config.desc}</p>{/if}
        </div>
      </div>
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
  {:else}
    <section class="setting-card setting-card-row" data-settings-search-id={config.id ?? undefined}>
      {#if config.icon}
        <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
      {/if}
      <span class="setting-label">{config.label}</span>
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
  {/if}
{:else if config.type === "text"}
  {#if (config.variant ?? "card") === "card"}
    <section class="setting-card" data-settings-search-id={config.id ?? undefined}>
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
    <section class="setting-card setting-card-row" data-settings-search-id={config.id ?? undefined}>
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
{:else if config.type === "size"}
  <section class="setting-card setting-card-row" data-settings-search-id={config.id ?? undefined}>
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
{:else if config.type === "custom"}
  {#if (config.variant ?? "toggle") === "toggle"}
    <section class="setting-card toggle-card" data-settings-search-id={config.id ?? undefined}>
      <div class="setting-heading">
        {#if config.icon}
          <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
        {/if}
        <div>
          <strong>{config.label}</strong>
          {#if config.desc}<p>{config.desc}</p>{/if}
        </div>
      </div>
      {@render children?.()}
    </section>
  {:else}
    <section
      class="setting-card setting-card-custom-column"
      data-settings-search-id={config.id ?? undefined}
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
      {@render children?.()}
    </section>
  {/if}
{:else}
  {#if (config.variant ?? "row") === "card"}
    <section class="setting-card" data-settings-search-id={config.id ?? undefined}>
      <div class="setting-heading">
        {#if config.icon}
          <span class="setting-icon"><AppIcon name={config.icon} size={17} /></span>
        {/if}
        <div class="heading-inline">
          <div>
            <strong>{config.label}</strong>
            {#if config.desc}<p>{config.desc}</p>{/if}
          </div>
          {#if config.suffix}<span class="value-label">{config.get()}{config.suffix}</span>{/if}
        </div>
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
    <section class="setting-card setting-card-row" data-settings-search-id={config.id ?? undefined}>
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
{/if}

<style>
  .setting-card-custom-column {
    display: flex;
    flex-direction: column;
    align-items: stretch;
  }

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
