/**
 * Promise-based themed confirmation dialog state.
 *
 * `confirmDialog()` queues a request and resolves once the user answers; the
 * `<ConfirmDialog />` host component (mounted once per window) renders it.
 * Callers must run in a window whose root mounts the host — the settings
 * window mounts it in its route root.
 */

export interface ConfirmOptions {
  title: string;
  message: string;
  /** Defaults to the generic confirm label. */
  confirmLabel?: string;
  /** Defaults to the generic cancel label. */
  cancelLabel?: string;
  /** Styles the confirm button as destructive. */
  danger?: boolean;
}

interface PendingRequest extends ConfirmOptions {
  resolve: (confirmed: boolean) => void;
}

let current = $state<PendingRequest | null>(null);

/** An optional owner signal cancels only this request, never a newer one. */
export function confirmDialog(options: ConfirmOptions, signal?: AbortSignal): Promise<boolean> {
  if (signal?.aborted) return Promise.resolve(false);
  return new Promise<boolean>((resolve) => {
    // A newer request replaces a still-pending one; the stale caller is
    // released as cancelled so nothing hangs.
    current?.resolve(false);
    const cancel = () => {
      if (current?.resolve === complete) settleConfirm(false);
    };
    const complete = (answer: boolean) => {
      signal?.removeEventListener("abort", cancel);
      resolve(answer);
    };
    current = { ...options, resolve: complete };
    signal?.addEventListener("abort", cancel, { once: true });
  });
}

export function currentConfirm(): PendingRequest | null {
  return current;
}

export function settleConfirm(confirmed: boolean): void {
  current?.resolve(confirmed);
  current = null;
}
