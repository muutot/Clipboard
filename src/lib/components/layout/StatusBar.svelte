<script lang="ts">
  interface Props {
    resultSummary: string;
    runtimeLabel: string;
    statusMessage: string;
  }

  let { resultSummary, runtimeLabel, statusMessage }: Props = $props();
</script>

<!-- The record count is the only dynamically loaded text in this bar: it grows
     while more records page in and it is replaced by the "searching" summary
     during an index search. It therefore lives at the right edge, where a
     width change only moves its own leading edge, instead of at the left,
     where it used to push the runtime label and the status message along
     with it. -->
<footer class="status-bar" role="status" aria-live="polite">
  <span class="status-left">
    <span class="runtime-status"><i></i>{runtimeLabel}</span>
  </span>
  <span class="status-msg">{statusMessage}</span>
  <span class="result-count">{resultSummary}</span>
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
    flex: 0 0 auto;
    color: var(--text-faint);
    white-space: nowrap;
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
