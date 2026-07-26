<script lang="ts">
  import { generalSettings } from "$lib/services/settings";
  import { onToast, type ToastType } from "$lib/services/toast";

  interface ToastEntry {
    id: number;
    message: string;
    type: ToastType;
    duration: number;
  }

  interface ToastState {
    entry: ToastEntry;
    leaving: boolean;
  }

  let toasts = $state<ToastState[]>([]);
  let toastNotificationsEnabled = $state(true);

  function removeToast(id: number) {
    const t = toasts.find((t) => t.entry.id === id);
    if (t && !t.leaving) {
      t.leaving = true;
      setTimeout(() => {
        toasts = toasts.filter((t) => t.entry.id !== id);
      }, 200);
    }
  }

  $effect(() => {
    return generalSettings.subscribe((settings) => {
      toastNotificationsEnabled = settings.showToastNotifications;
      if (!toastNotificationsEnabled) toasts = [];
    });
  });

  $effect(() => {
    return onToast((entry) => {
      if (!toastNotificationsEnabled) return;
      toasts = [...toasts, { entry, leaving: false }];
      setTimeout(() => removeToast(entry.id), entry.duration);
    });
  });
</script>

{#if toasts.length > 0}
  <div class="toast-container" aria-live="polite">
    {#each toasts as toast (toast.entry.id)}
      <div class="toast toast-{toast.entry.type}" class:toast-leaving={toast.leaving}>
        <span class="toast-icon">
          {#if toast.entry.type === "success"}✓
          {:else if toast.entry.type === "error"}✗
          {:else}ℹ
          {/if}
        </span>
        <span class="toast-message">{toast.entry.message}</span>
      </div>
    {/each}
  </div>
{/if}

<style>
  .toast-container {
    position: fixed;
    z-index: 9999;
    bottom: 60px;
    left: 50%;
    transform: translateX(-50%);
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }

  .toast {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    padding: 9px 18px;
    border-radius: 10px;
    color: var(--text-primary);
    font-size: 13px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.45);
    pointer-events: auto;
    animation: toast-in 220ms ease-out forwards;
  }

  .toast.toast-leaving {
    animation: toast-out 180ms ease-in forwards;
  }

  .toast-success {
    background: color-mix(in srgb, var(--success-color) 15%, var(--surface-bg));
    border: 1px solid color-mix(in srgb, var(--success-color) 35%, transparent);
  }

  .toast-error {
    background: color-mix(in srgb, var(--danger-color) 15%, var(--surface-bg));
    border: 1px solid color-mix(in srgb, var(--danger-color) 35%, transparent);
  }

  .toast-info {
    background: color-mix(in srgb, var(--selection-color) 15%, var(--surface-bg));
    border: 1px solid color-mix(in srgb, var(--selection-color) 35%, transparent);
  }

  .toast-icon {
    font-size: 14px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .toast-success .toast-icon {
    color: var(--success-color);
  }
  .toast-error .toast-icon {
    color: var(--danger-color);
  }
  .toast-info .toast-icon {
    color: var(--selection-color);
  }

  .toast-message {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @keyframes toast-in {
    from {
      opacity: 0;
      transform: translateY(14px) scale(0.94);
    }
    to {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
  }

  @keyframes toast-out {
    from {
      opacity: 1;
      transform: translateY(0) scale(1);
    }
    to {
      opacity: 0;
      transform: translateY(-8px) scale(0.96);
    }
  }
</style>
