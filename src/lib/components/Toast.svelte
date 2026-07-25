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
    color: #f1f1f1;
    font-size: 13px;
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.45);
    pointer-events: auto;
    animation: toast-in 220ms ease-out forwards;
  }

  .toast.toast-leaving {
    animation: toast-out 180ms ease-in forwards;
  }

  .toast-success {
    background: #1c3a28;
    border: 1px solid #2d5a3d;
  }

  .toast-error {
    background: #3a1c1c;
    border: 1px solid #5a2d2d;
  }

  .toast-info {
    background: #1c2a3a;
    border: 1px solid #2d3d5a;
  }

  .toast-icon {
    font-size: 14px;
    font-weight: 700;
    flex-shrink: 0;
  }

  .toast-success .toast-icon {
    color: #4ec96a;
  }
  .toast-error .toast-icon {
    color: #e85d5d;
  }
  .toast-info .toast-icon {
    color: #5d9ee8;
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
