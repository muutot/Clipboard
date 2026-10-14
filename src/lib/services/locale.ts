import { invokeTauri } from "$lib/services/runtime";

/**
 * OS UI locale name (`zh-CN`, `en-US`, …) reported by the desktop backend.
 * Returns `null` outside the Tauri runtime or when the platform call fails —
 * callers then fall back to the webview's `navigator.language`, which follows
 * the Chromium accepted-language list and can disagree with the OS display
 * language (Chinese Windows with an English preferred-language list), so this
 * answer is preferred whenever it exists.
 */
export async function getOsLocaleName(): Promise<string | null> {
  try {
    return await invokeTauri<string | null>("get_os_locale");
  } catch (error) {
    console.warn("Unable to read the OS locale", error);
    return null;
  }
}
