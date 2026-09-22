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

// Tracks how many menus are visibly open so the main route can yield Escape
// to the menu: a window-level `stopPropagation` cannot do this because the
// route's capture-phase selection clear and its bubble-phase global handler
// (registered before the menu mounts) both run regardless. Cards report via
// `trackContextMenuOpen` in an `$effect` so every close path (Escape,
// outside click, action, bus close, unmount) balances through the cleanup.
let openCount = 0;
let openChangedListeners: Array<(open: boolean) => void> = [];

function emitOpenChanged(): void {
  const open = openCount > 0;
  for (const listener of [...openChangedListeners]) {
    listener(open);
  }
}

export function trackContextMenuOpen(): () => void {
  openCount += 1;
  emitOpenChanged();
  let released = false;
  return () => {
    if (released) return;
    released = true;
    openCount = Math.max(0, openCount - 1);
    emitOpenChanged();
  };
}

export function onContextMenuOpenChanged(callback: (open: boolean) => void): () => void {
  openChangedListeners = [...openChangedListeners, callback];
  return () => {
    openChangedListeners = openChangedListeners.filter((fn) => fn !== callback);
  };
}
