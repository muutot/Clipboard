import { getCurrentWindow } from "@tauri-apps/api/window";
import { get } from "svelte/store";
import { generalSettings, restoreWindowPosition, saveWindowPosition } from "$lib/services/settings";
import { isTauriRuntime } from "$lib/services/runtime";
import { applyGeneralSettingsToDocument } from "$lib/services/settings-bootstrap";
import { createWindowBoundsController } from "$lib/utils/window-bounds";

/** Handles both already-registered listeners and registrations that resolve after disposal. */
export function createListenerScope() {
  let disposed = false;
  const listeners = new Set<() => void>();
  return {
    add(registration: Promise<() => void>) {
      void registration
        .then((unlisten) => {
          if (disposed) unlisten();
          else listeners.add(unlisten);
        })
        .catch(() => {});
    },
    dispose() {
      disposed = true;
      for (const unlisten of listeners) unlisten();
      listeners.clear();
    },
  };
}

/** Mounted window settings, geometry persistence and native listeners have one disposal owner. */
export function createMainWindowController(onFocus: () => Promise<void>) {
  const appWindow = isTauriRuntime() ? getCurrentWindow() : null;
  let previousRememberWindowPosition = false;
  const scope = createListenerScope();
  const bounds = createWindowBoundsController({
    appWindow,
    isRemembered: () => get(generalSettings).rememberWindowPosition,
    savePosition: saveWindowPosition,
    restorePosition: restoreWindowPosition,
    storage: typeof window !== "undefined" ? window.localStorage : null,
  });
  const unsubscribe = generalSettings.subscribe((settings) => {
    applyGeneralSettingsToDocument(settings);
    if (appWindow) {
      appWindow.setAlwaysOnTop(settings.alwaysOnTop).catch(() => {});
      appWindow.setDecorations(settings.useSystemTitleBar).catch(() => {});
      if (!settings.rememberWindowPosition) bounds.resetRestoreAttempt();
      else if (!previousRememberWindowPosition) void bounds.restore();
    }
    previousRememberWindowPosition = settings.rememberWindowPosition;
  });
  if (appWindow) {
    scope.add(
      appWindow.onFocusChanged(() => {
        void onFocus();
      }),
    );
    scope.add(appWindow.onMoved(() => bounds.scheduleSave()));
    scope.add(appWindow.onResized(() => bounds.scheduleSave()));
  }
  return {
    dispose() {
      scope.dispose();
      unsubscribe();
      void bounds.flush();
    },
  };
}
