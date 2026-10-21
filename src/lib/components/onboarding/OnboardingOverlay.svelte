<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";
  import { messages, resolvePath } from "$lib/i18n";
  import { onMount } from "svelte";
  import { trapTabFocus } from "$lib/utils/keyboard/focus";
  import type { IconName } from "$lib/types/clipboard";

  interface Props {
    onclose: () => void;
  }

  let { onclose }: Props = $props();

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  type StepKey = "welcome" | "search" | "cards" | "organize" | "privacy" | "power";

  const steps: { key: StepKey; icon: IconName }[] = [
    { key: "welcome", icon: "clipboard" },
    { key: "search", icon: "search" },
    { key: "cards", icon: "copy" },
    { key: "organize", icon: "filter" },
    { key: "privacy", icon: "lock" },
    { key: "power", icon: "sliders" },
  ];

  let step = $state(0);
  let panelEl = $state<HTMLElement | null>(null);

  const current = $derived($messages.onboarding.steps[steps[step].key]);
  const isLast = $derived(step === steps.length - 1);

  onMount(() => {
    panelEl?.querySelector<HTMLButtonElement>("[data-autofocus]")?.focus();
  });

  function next() {
    if (isLast) {
      onclose();
      return;
    }
    step += 1;
  }

  function previous() {
    if (step > 0) step -= 1;
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Tab") {
      trapTabFocus(panelEl, event);
      return;
    }
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation();
      onclose();
      return;
    }
    if (event.key === "ArrowRight") {
      event.preventDefault();
      event.stopPropagation();
      next();
      return;
    }
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      event.stopPropagation();
      previous();
    }
  }
</script>

<div class="onboarding-backdrop" aria-hidden="true"></div>
<div
  class="onboarding-panel"
  role="dialog"
  aria-modal="true"
  aria-labelledby="onboarding-title"
  bind:this={panelEl}
  onkeydowncapture={handleKeydown}
>
  <header class="onboarding-header">
    <span class="onboarding-eyebrow">{_t("onboarding.eyebrow")}</span>
    <button
      class="onboarding-close"
      type="button"
      aria-label={_t("onboarding.skip")}
      onclick={onclose}>×</button
    >
  </header>

  <div class="onboarding-body">
    <span class="onboarding-icon"><AppIcon name={steps[step].icon} size={24} /></span>
    <h2 id="onboarding-title">{current.title}</h2>
    <p class="onboarding-desc">{current.desc}</p>
    <ul class="onboarding-points">
      {#each current.points as point}
        <li>{point}</li>
      {/each}
    </ul>
  </div>

  <footer class="onboarding-footer">
    <span class="onboarding-progress">
      {_t("onboarding.progress", { current: step + 1, total: steps.length })}
    </span>
    <div class="onboarding-dots" aria-hidden="true">
      {#each steps as _, index}
        <span class="onboarding-dot" class:active={index === step}></span>
      {/each}
    </div>
    <div class="onboarding-actions">
      <button class="onboarding-btn" type="button" onclick={onclose}>{_t("onboarding.skip")}</button
      >
      <button class="onboarding-btn" type="button" disabled={step === 0} onclick={previous}>
        {_t("onboarding.prev")}
      </button>
      <button class="onboarding-btn primary" type="button" data-autofocus onclick={next}>
        {isLast ? _t("onboarding.finish") : _t("onboarding.next")}
      </button>
    </div>
  </footer>
</div>

<style>
  .onboarding-backdrop {
    position: fixed;
    z-index: 300;
    inset: 0;
    background: rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(4px);
    pointer-events: none;
  }

  .onboarding-panel {
    position: fixed;
    z-index: 301;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: min(560px, calc(100vw - 32px));
    max-height: min(620px, calc(100vh - 32px));
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border: 1px solid var(--border-color);
    border-radius: 12px;
    background: var(--surface-bg);
    box-shadow: 0 18px 48px rgba(0, 0, 0, 0.45);
    color: var(--text-primary);
    animation: onboarding-in 180ms ease-out;
  }

  @keyframes onboarding-in {
    from {
      opacity: 0;
      transform: translate(-50%, calc(-50% + 8px));
    }
    to {
      opacity: 1;
      transform: translate(-50%, -50%);
    }
  }

  .onboarding-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 14px 18px 0;
  }

  .onboarding-eyebrow {
    color: var(--text-muted);
    font-size: var(--font-size-tiny);
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .onboarding-close {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text-muted);
    font-size: 19px;
    line-height: 1;
    cursor: pointer;
  }

  .onboarding-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }

  .onboarding-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 12px 22px 18px;
  }

  .onboarding-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 42px;
    height: 42px;
    border-radius: 10px;
    background: color-mix(in srgb, var(--selection-color) 14%, transparent);
    color: var(--selection-color);
  }

  .onboarding-body h2 {
    margin: 12px 0 0;
    font-size: var(--font-size-base);
    font-weight: 600;
  }

  .onboarding-desc {
    margin: 6px 0 0;
    color: var(--text-secondary);
    font-size: var(--font-size-secondary);
    line-height: 1.6;
  }

  .onboarding-points {
    margin: 14px 0 0;
    padding: 0;
    list-style: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .onboarding-points li {
    position: relative;
    padding-left: 16px;
    color: var(--text-secondary);
    font-size: var(--font-size-secondary);
    line-height: 1.6;
  }

  .onboarding-points li::before {
    content: "";
    position: absolute;
    top: 0.62em;
    left: 2px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .onboarding-footer {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 18px;
    border-top: 1px solid var(--border-subtle);
    background: var(--statusbar-bg);
  }

  .onboarding-progress {
    color: var(--text-muted);
    font-size: var(--font-size-tiny);
    white-space: nowrap;
  }

  .onboarding-dots {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-right: auto;
  }

  .onboarding-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--border-color);
  }

  .onboarding-dot.active {
    background: var(--selection-color);
  }

  .onboarding-actions {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .onboarding-btn {
    height: 30px;
    box-sizing: border-box;
    padding: 5px 14px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    background: var(--hover-bg);
    color: var(--text-secondary);
    font: inherit;
    font-size: var(--font-size-secondary);
    white-space: nowrap;
    cursor: pointer;
    transition:
      background 100ms ease,
      color 100ms ease;
  }

  .onboarding-btn:hover {
    color: var(--text-primary);
  }

  .onboarding-btn:disabled {
    opacity: 0.55;
    cursor: default;
  }

  .onboarding-btn.primary {
    border-color: var(--selection-color);
    background: var(--selection-color);
    color: #ffffff;
  }

  .onboarding-btn.primary:hover {
    filter: brightness(1.08);
    color: #ffffff;
  }
</style>
