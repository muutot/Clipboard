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

export function confirmDialog(options: ConfirmOptions): Promise<boolean> {
  return new Promise<boolean>((resolve) => {
    // A newer request replaces a still-pending one; the stale caller is
    // released as cancelled so nothing hangs.
    current?.resolve(false);
    current = { ...options, resolve };
  });
}

export function currentConfirm(): PendingRequest | null {
  return current;
}

export function settleConfirm(confirmed: boolean): void {
  current?.resolve(confirmed);
  current = null;
}
