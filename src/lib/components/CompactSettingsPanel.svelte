<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { generalSettings } from "$lib/services/settings";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    onclose: () => void;
    showHeader?: boolean;
  }

  let { onclose, showHeader = true }: Props = $props();

  let s = $state($generalSettings);
  $effect(() => {
    const unsub = generalSettings.subscribe((v) => {
      s = v;
    });
    return unsub;
  });

  function sliderPercentage(value: number, min: number, max: number): string {
    const percentage = ((value - min) / (max - min)) * 100;
    return `${Math.min(100, Math.max(0, percentage))}%`;
  }

  function sliderHandler(key: keyof typeof s) {
    return (event: Event) => {
      const val = Number((event.target as HTMLInputElement).value);
      generalSettings.updateSetting(key, val);
    };
  }

  const compactSliders = [
    {
      key: "compactPaddingTop" as const,
      icon: "sliders" as const,
      label: "compact.paddingTop",
      min: 0,
      max: 20,
    },
    {
      key: "compactPaddingBottom" as const,
      icon: "sliders" as const,
      label: "compact.paddingBottom",
      min: 0,
      max: 20,
    },
    { key: "compactCardGap" as const, icon: "ruler", label: "compact.cardGap", min: 0, max: 20 },
    {
      key: "compactTextHeight" as const,
      icon: "text",
      label: "compact.shortTextHeight",
      min: 36,
      max: 90,
    },
    {
      key: "compactTallTextHeight" as const,
      icon: "text",
      label: "compact.tallTextHeight",
      min: 44,
      max: 100,
    },
    {
      key: "compactImageHeight" as const,
      icon: "image",
      label: "compact.imageHeight",
      min: 64,
      max: 200,
    },
    {
      key: "compactSearchHeight" as const,
      icon: "search",
      label: "compact.searchHeight",
      min: 28,
      max: 56,
    },
    {
      key: "compactSearchFontSize" as const,
      icon: "type",
      label: "compact.searchFontSize",
      min: 10,
      max: 24,
    },
    {
      key: "compactCardBorderRadius" as const,
      icon: "grid",
      label: "compact.cardBorderRadius",
      min: 0,
      max: 20,
    },
  ];
</script>

{#if showHeader}
  <header>
    <div>
      <span class="eyebrow">{_t("compact.eyebrow")}</span>
      <h2>{_t("compact.title")}</h2>
      <p>{_t("compact.description")}</p>
    </div>
    <button class="close-button" type="button" aria-label={_t("actions.close")} onclick={onclose}
      >×</button
    >
  </header>
{/if}

<div class="settings-scroll">
  <section class="setting-card toggle-card">
    <div class="setting-heading">
      <span class="setting-icon"><AppIcon name="grid" size={17} /></span>
      <div>
        <strong>{_t("general.compactMode")}</strong>
        <p>{_t("general.compactModeDescription")}</p>
      </div>
    </div>
    <button
      type="button"
      class="toggle-switch"
      class:active={s.compactMode}
      onclick={() => generalSettings.updateSetting("compactMode", !s.compactMode)}
      aria-checked={s.compactMode}
      aria-label={_t("general.compactMode")}
      role="switch"
    >
      <span class="toggle-knob"></span>
    </button>
  </section>

  {#if s.compactMode}
    {#each compactSliders as slider}
      <section class="setting-card">
        <div class="setting-heading">
          <span class="setting-icon"><AppIcon name={slider.icon as any} size={17} /></span>
          <div class="heading-inline">
            <div>
              <strong>{_t(slider.label)}</strong>
              <p>{_t(`${slider.label}Description`)}</p>
            </div>
            <span class="value-label">{s[slider.key]}px</span>
          </div>
        </div>
        <input
          type="range"
          min={slider.min}
          max={slider.max}
          value={s[slider.key] as number}
          oninput={sliderHandler(slider.key)}
          class="transparency-slider"
          style:--slider-pct={sliderPercentage(s[slider.key] as number, slider.min, slider.max)}
        />
      </section>
    {/each}
  {/if}

  <p class="auto-save-note">{_t("general.autoSaveNote")}</p>
</div>
