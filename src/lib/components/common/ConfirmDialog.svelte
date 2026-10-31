<script lang="ts">
  import { currentConfirm, settleConfirm } from "$lib/services/ui/confirm.svelte";
  import { t } from "$lib/i18n";

  let dialog = $state<HTMLDialogElement | null>(null);
  let confirmButton = $state<HTMLButtonElement | null>(null);

  const request = $derived(currentConfirm());

  $effect(() => {
    if (request && dialog && !dialog.open) {
      dialog.showModal();
      confirmButton?.focus();
    }
  });

  function handleCancel(event: Event) {
    // Escape (and the native cancel path) must settle the pending promise
    // instead of aborting the modal, otherwise callers would hang.
    event.preventDefault();
    settleConfirm(false);
  }

  function handleClose() {
    if (currentConfirm()) settleConfirm(false);
  }

  function handleBackdropClick(event: MouseEvent) {
    if (event.target === event.currentTarget) settleConfirm(false);
  }
</script>

{#if request}
  <dialog
    bind:this={dialog}
    class="confirm-dialog"
    aria-label={request.title}
    onclick={handleBackdropClick}
    oncancel={handleCancel}
    onclose={handleClose}
  >
    <div class="confirm-content">
      <div class="confirm-title">{request.title}</div>
      <div class="confirm-message">{request.message}</div>
      <div class="confirm-actions">
        <button type="button" class="confirm-cancel" onclick={() => settleConfirm(false)}>
          {request.cancelLabel ?? t("confirm.cancel")}
        </button>
        <button
          type="button"
          bind:this={confirmButton}
          class="confirm-accept"
          class:danger={request.danger}
          onclick={() => settleConfirm(true)}
        >
          {request.confirmLabel ?? t("confirm.confirm")}
        </button>
      </div>
    </div>
  </dialog>
{/if}

<style>
  .confirm-dialog {
    margin: auto;
    padding: 0;
    border: 1px solid var(--border-color);
    border-radius: 10px;
    color: var(--text-primary);
    background: var(--card-bg);
    box-shadow: 0 8px 32px rgba(0, 0, 0, 0.5);
    width: min(320px, calc(100vw - 32px));
  }

  .confirm-dialog::backdrop {
    background: rgba(0, 0, 0, 0.52);
    backdrop-filter: blur(2px);
  }

  .confirm-content {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 14px;
  }

  .confirm-title {
    color: var(--text-primary);
    font-size: 13px;
    font-weight: 600;
  }

  .confirm-message {
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.5;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }

  .confirm-actions {
    display: flex;
    justify-content: flex-end;
    gap: 8px;
    margin-top: 2px;
  }

  .confirm-actions button {
    padding: 5px 14px;
    border: 1px solid var(--border-color);
    border-radius: 6px;
    color: var(--text-primary);
    background: var(--input-bg);
    font-size: 12px;
    cursor: pointer;
  }

  .confirm-cancel:hover {
    border-color: var(--text-faint);
  }

  /* Match the app's primary action styling (DetailEditActions edit-save). */
  .confirm-accept {
    color: var(--text-primary);
    background: var(--hover-bg);
    border-color: var(--text-faint);
  }

  .confirm-accept:hover {
    color: #ffffff;
    background: var(--border-color);
  }

  .confirm-accept.danger {
    color: #ffffff;
    background: var(--danger-color);
    border-color: var(--danger-color);
  }

  .confirm-accept.danger:hover {
    filter: brightness(1.08);
  }

  .confirm-actions button:focus-visible {
    outline: none;
    border-color: var(--text-faint);
  }
</style>
