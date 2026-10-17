import { invokeTauriRequired } from "$lib/services/runtime";

/**
 * Main-window visibility goes through the backend so the WebView2 lifecycle
 * and native visibility cannot drift. The commands require the desktop app.
 */
export async function showMainWindow(): Promise<void> {
  await invokeTauriRequired<unknown>(
    "show_main_window_command",
    undefined,
    "Main-window controls require the desktop app",
  );
}

export async function hideMainWindow(): Promise<void> {
  await invokeTauriRequired<unknown>(
    "hide_main_window_command",
    undefined,
    "Main-window controls require the desktop app",
  );
}
