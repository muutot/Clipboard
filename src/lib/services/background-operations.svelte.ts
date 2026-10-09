import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "./runtime";

export type OperationKind = "tags" | "backup" | "sync";
export interface OperationSnapshot {
  id: string;
  kind: OperationKind;
  status: "running" | "cancelling" | "succeeded" | "failed" | "cancelled";
  phase:
    | "preparing"
    | "scanning"
    | "applying"
    | "creating"
    | "validating"
    | "restoring"
    | "transferring"
    | "discovering"
    | "uploading"
    | "downloading"
    | "compacting";
  completed: number;
  total: number | null;
}
export function isOperationCancelled(error: unknown): boolean {
  const text = String(error);
  return text.includes("operation cancelled") || text.includes("sync cancelled");
}

/** Each mounted panel polls one bounded backend slot; remounts recover active work. */
export function createOperationMonitor(kind: OperationKind) {
  let snapshot = $state<OperationSnapshot | null>(null);
  let error = $state("");
  let cancelling = $state(false);
  let disposed = false;
  let generation = 0;
  async function refresh() {
    if (!isTauriRuntime() || disposed) return;
    const request = ++generation;
    try {
      const current = await invoke<OperationSnapshot | null>("get_background_operation", { kind });
      if (!disposed && request === generation) {
        snapshot = current;
        error = "";
      }
    } catch (failure) {
      if (!disposed && request === generation) error = String(failure);
    }
  }
  $effect(() => {
    disposed = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let stopped = false;
    async function poll() {
      await refresh();
      if (!stopped && isTauriRuntime()) timer = setTimeout(poll, 500);
    }
    void poll();
    return () => {
      stopped = true;
      disposed = true;
      generation++;
      clearTimeout(timer);
    };
  });
  async function cancel() {
    if (!snapshot || cancelling) return;
    const id = snapshot.id;
    cancelling = true;
    try {
      await invoke<boolean>("cancel_background_operation", { kind, id });
      await refresh();
    } catch (failure) {
      if (!disposed) error = String(failure);
    } finally {
      if (!disposed) cancelling = false;
    }
  }
  return {
    get snapshot() {
      return snapshot;
    },
    get active() {
      return snapshot?.status === "running" || snapshot?.status === "cancelling";
    },
    get error() {
      return error;
    },
    get cancelling() {
      return cancelling;
    },
    refresh,
    cancel,
  };
}
