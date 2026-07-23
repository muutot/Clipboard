export type ToastType = "success" | "error" | "info";

export interface ToastEntry {
  id: number;
  message: string;
  type: ToastType;
  duration: number;
}

let nextId = 0;
let listeners: Array<(toast: ToastEntry) => void> = [];

export function onToast(callback: (toast: ToastEntry) => void): () => void {
  listeners = [...listeners, callback];
  return () => {
    listeners = listeners.filter((fn) => fn !== callback);
  };
}

export function showToast(message: string, type: ToastType = "info", duration = 3000): void {
  const toast: ToastEntry = { id: ++nextId, message, type, duration };
  for (const listener of listeners) {
    listener(toast);
  }
}
