let closeListeners: Array<() => void> = [];

export function onContextMenuOpened(callback: () => void): () => void {
  closeListeners = [...closeListeners, callback];
  return () => {
    closeListeners = closeListeners.filter((fn) => fn !== callback);
  };
}

export function notifyContextMenuOpened(): void {
  for (const listener of closeListeners) {
    listener();
  }
}
