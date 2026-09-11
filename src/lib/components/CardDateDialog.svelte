<script lang="ts">
  import AppIcon from "$lib/components/AppIcon.svelte";

  interface DateView {
    isoDate: string;
    formattedDate: string;
    label: string;
  }

  interface Props {
    dateView: DateView | null;
    ondismiss: () => void;
  }

  let { dateView, ondismiss }: Props = $props();

  let dialogEl = $state<HTMLDialogElement | null>(null);

  // Open/close reactively so the parent only owns the date payload.
  $effect(() => {
    const shouldOpen = dateView !== null;
    const el = dialogEl;
    if (!el) return;
    if (shouldOpen && !el.open) {
      try {
        el.showModal();
      } catch {
        el.setAttribute("open", "");
      }
    } else if (!shouldOpen && el.open) {
      try {
        el.close();
      } catch {
        el.removeAttribute("open");
      }
    }
  });

  function dismiss() {
    ondismiss();
  }

  function handleKeydown(event: KeyboardEvent) {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    dismiss();
  }

  function handleClick(event: MouseEvent) {
    if (event.target === event.currentTarget) dismiss();
  }
</script>

<dialog
  bind:this={dialogEl}
  class="date-action-dialog"
  aria-label={dateView?.label ?? "View date"}
  onclose={ondismiss}
  onclick={handleClick}
  onkeydown={handleKeydown}
>
  {#if dateView}
    <div class="date-action-content">
      <span class="date-action-icon"><AppIcon name="calendar" size={20} /></span>
      <div class="date-action-text">
        <time datetime={dateView.isoDate}>{dateView.formattedDate}</time>
        <span>{dateView.isoDate}</span>
      </div>
      <button
        type="button"
        class="date-action-close"
        title="Close date"
        aria-label="Close date"
        onclick={dismiss}
      >
        <AppIcon name="x" size={15} />
      </button>
    </div>
  {/if}
</dialog>

<style>
  .date-action-dialog {
    position: fixed;
    width: min(320px, calc(100vw - 32px));
    margin: auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 8px;
    color: var(--text-primary);
    background: var(--card-bg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
  }

  .date-action-dialog::backdrop {
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(2px);
  }

  .date-action-content {
    position: relative;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 58px;
    padding: 12px 38px 12px 14px;
  }

  .date-action-icon {
    display: inline-flex;
    flex: 0 0 auto;
    align-items: center;
    justify-content: center;
    width: 28px;
    height: 28px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--warning-color);
    background: var(--input-bg);
  }

  .date-action-text {
    display: flex;
    min-width: 0;
    flex-direction: column;
    gap: 3px;
  }

  .date-action-text time {
    color: var(--text-primary);
    font-size: 13px;
    line-height: 1.35;
  }

  .date-action-text span {
    color: var(--text-muted);
    font-family: ui-monospace, SFMono-Regular, Consolas, monospace;
    font-size: 11px;
    line-height: 1.3;
  }

  .date-action-close {
    position: absolute;
    top: 8px;
    right: 8px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 5px;
    color: var(--text-muted);
    background: transparent;
    cursor: pointer;
  }

  .date-action-close:hover {
    color: var(--text-primary);
    background: var(--hover-bg);
  }
</style>
