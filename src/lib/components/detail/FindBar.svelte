<script lang="ts">
  import AppIcon from "$lib/components/card/AppIcon.svelte";

  interface Props {
    query: string;
    count: number;
    current: number;
    placeholder: string;
    label: string;
    prevTitle: string;
    nextTitle: string;
    closeTitle: string;
    onquerychange: (query: string) => void;
    onprev: () => void;
    onnext: () => void;
    onclose: () => void;
  }

  let {
    query,
    count,
    current,
    placeholder,
    label,
    prevTitle,
    nextTitle,
    closeTitle,
    onquerychange,
    onprev,
    onnext,
    onclose,
  }: Props = $props();

  let input = $state<HTMLInputElement | null>(null);

  export function focusInput() {
    input?.focus();
    input?.select();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key === "Enter") {
      event.preventDefault();
      if (event.shiftKey) onprev();
      else onnext();
    }
  }
</script>

<div class="find-bar" role="search" aria-label={label}>
  <input
    bind:this={input}
    bind:value={query}
    oninput={() => onquerychange(query)}
    onkeydown={handleKeydown}
    type="text"
    {placeholder}
    aria-label={label}
  />
  <span class="match-count" class:empty={count === 0}>{current + 1}/{count}</span>
  <button
    type="button"
    class="nav-button"
    title={prevTitle}
    onclick={onprev}
    disabled={count === 0}
  >
    <AppIcon name="chevron-left" size={13} />
  </button>
  <button
    type="button"
    class="nav-button"
    title={nextTitle}
    onclick={onnext}
    disabled={count === 0}
  >
    <AppIcon name="chevron-right" size={13} />
  </button>
  <button type="button" class="nav-button" title={closeTitle} onclick={onclose}>
    <AppIcon name="x" size={13} />
  </button>
</div>

<style>
  .find-bar {
    position: absolute;
    top: 6px;
    right: 10px;
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 6px;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    background: var(--surface-bg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    backdrop-filter: blur(12px);
  }

  .find-bar input {
    width: 150px;
    padding: 3px 8px;
    border: 1px solid var(--selection-color);
    border-radius: 5px;
    color: var(--text-primary);
    background: var(--input-bg);
    font-size: 12.5px;
    outline: none;
  }

  .find-bar input::placeholder {
    color: var(--placeholder-color);
  }

  .match-count {
    min-width: 38px;
    color: var(--text-secondary);
    font-size: 11px;
    text-align: center;
    white-space: nowrap;
  }

  .match-count.empty {
    color: var(--text-muted);
  }

  .nav-button {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
  }

  .nav-button:hover:not(:disabled) {
    background: var(--hover-bg);
    color: var(--text-primary);
  }

  .nav-button:disabled {
    color: var(--text-faint);
    cursor: default;
  }
</style>
