<script lang="ts">
  import { messages, resolvePath } from "$lib/i18n";

  const _t = (path: string, params?: Record<string, string | number>) =>
    resolvePath($messages, path, params);

  interface Props {
    resultSummary: string;
    runtimeLabel: string;
    statusMessage: string;
    /** Current main-toggle binding (e.g. "Alt+C"), null when unbound. */
    toggleHint: string | null;
  }

  let { resultSummary, runtimeLabel, statusMessage, toggleHint }: Props = $props();

  const hintParts = $derived(toggleHint === null ? [] : toggleHint.split("+"));
</script>

<footer class="status-bar" role="status" aria-live="polite">
  <span class="status-left">
    <span class="result-count">{resultSummary}</span>
    <span class="runtime-status"><i></i>{runtimeLabel}</span>
  </span>
  <span class="status-msg">{statusMessage}</span>
  {#if hintParts.length > 0}
    <span class="shortcut-hints">
      {#each hintParts as part, index (index)}
        {#if index > 0}<b>+</b>{/if}<kbd>{part}</kbd>
      {/each}
      {_t("app.shortcutHint")}</span
    >
  {/if}
</footer>

<style>
  .status-bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    min-height: 34px;
    padding: 7px 14px;
    border-top: 1px solid var(--border-subtle);
    color: var(--text-faint);
    background: var(--statusbar-bg);
    font-size: 11.5px;
    overflow: hidden;
  }

  .status-left {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-shrink: 0;
  }

  .result-count {
    color: var(--text-faint);
  }

  .runtime-status {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }

  .runtime-status i {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--success-color);
    box-shadow: 0 0 8px color-mix(in srgb, var(--success-color) 40%, transparent);
  }

  .status-msg {
    flex: 1 1 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
    min-width: 0;
  }

  .shortcut-hints {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    gap: 4px;
  }

  .shortcut-hints kbd {
    padding: 1px 5px;
    border: 1px solid var(--border-color);
    border-radius: 4px;
    color: var(--text-muted);
    background: var(--hover-bg);
    box-shadow: 0 1px 0 rgba(0, 0, 0, 0.4);
    font: inherit;
  }

  .shortcut-hints b {
    font-weight: 400;
  }

  /* Spans the route's split-detail grid. This deliberately escapes component
     scope: the grid belongs to the route, and it is the only cross-scope
     rule for this bar. */
  :global(.app-shell.split-detail) > .status-bar {
    grid-column: 1 / -1;
  }

  /* Narrow-window behavior lives with the component: page-scoped selectors
     cannot reach inside it. Breakpoint mirrors the main toolbar (660px). */
  @media (max-width: 660px) {
    .status-bar > span:first-child {
      display: none;
    }
    .status-bar {
      justify-content: flex-end;
    }
  }
</style>
