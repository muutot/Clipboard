import { invoke } from "@tauri-apps/api/core";
import type { RuntimeInfo } from "$lib/types/clipboard";

export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

type InvokeArgs = Record<string, unknown>;

/**
 * Tauri invoke guarded by the runtime check. Outside the desktop app the
 * call never happens: without a fallback the result is `null`, otherwise
 * the fallback is returned. Collapses the three-line
 * `if (!isTauriRuntime()) return …` guard repeated across every service
 * module.
 */
export async function invokeTauri<T>(command: string, args?: InvokeArgs): Promise<T | null>;
export async function invokeTauri<T>(
  command: string,
  args: InvokeArgs | undefined,
  fallback: T,
): Promise<T>;
export async function invokeTauri<T>(
  command: string,
  args?: InvokeArgs,
  fallback: T | null = null,
): Promise<T | null> {
  if (!isTauriRuntime()) return fallback;
  return invoke<T>(command, args);
}

/**
 * Variant for operations that cannot proceed without the desktop backend:
 * throws `message` outside Tauri instead of returning a fallback.
 */
export async function invokeTauriRequired<T>(
  command: string,
  args: InvokeArgs | undefined,
  message: string,
): Promise<T> {
  if (!isTauriRuntime()) throw new Error(message);
  return invoke<T>(command, args);
}

export async function getRuntimeInfo(): Promise<RuntimeInfo | null> {
  try {
    return await invokeTauri<RuntimeInfo>("get_runtime_info");
  } catch (error) {
    console.warn("Unable to read Tauri runtime information", error);
    return null;
  }
}

/** Opens the calling window's WebView developer console (About panel, F12). */
export function openDevtools(): void {
  if (!isTauriRuntime()) return;
  invoke("open_devtools").catch((error) => console.warn("open_devtools failed:", error));
}

/** Whether a keydown event is the app-wide developer-console hotkey. */
export function isDevtoolsHotkey(event: KeyboardEvent): boolean {
  return event.key === "F12" && !event.ctrlKey && !event.altKey && !event.metaKey;
}
