import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";

export interface KeyboardConfig {
  shortcuts: Record<string, string[]>;
  [key: string]: unknown;
}

export async function getKeyboardConfig(): Promise<KeyboardConfig | null> {
  if (!isTauriRuntime()) return null;

  return invoke<KeyboardConfig>("get_keyboard_config");
}

export async function configureKeyboardShortcuts(
  action: string,
  shortcuts: string[],
): Promise<string[]> {
  if (!isTauriRuntime()) {
    throw new Error("Keyboard configuration is only available in the desktop app");
  }

  return invoke<string[]>("configure_keyboard_shortcuts", { action, shortcuts });
}

export async function deleteKeyboardAction(action: string): Promise<void> {
  if (!isTauriRuntime()) {
    throw new Error("Keyboard configuration is only available in the desktop app");
  }

  return invoke<void>("delete_keyboard_action", { action });
}

export async function resetKeyboardConfig(): Promise<KeyboardConfig> {
  if (!isTauriRuntime()) {
    throw new Error("Keyboard configuration is only available in the desktop app");
  }

  return invoke<KeyboardConfig>("reset_keyboard_config");
}
