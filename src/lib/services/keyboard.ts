import { invokeTauri, invokeTauriRequired } from "$lib/services/runtime";

export interface KeyboardConfig {
  shortcuts: Record<string, string[]>;
  [key: string]: unknown;
}

export async function getKeyboardConfig(): Promise<KeyboardConfig | null> {
  return invokeTauri<KeyboardConfig>("get_keyboard_config");
}

export async function configureKeyboardShortcuts(
  action: string,
  shortcuts: string[],
): Promise<string[]> {
  return invokeTauriRequired<string[]>(
    "configure_keyboard_shortcuts",
    { action, shortcuts },
    "Keyboard configuration is only available in the desktop app",
  );
}

export async function deleteKeyboardAction(action: string): Promise<void> {
  return invokeTauriRequired<void>(
    "delete_keyboard_action",
    { action },
    "Keyboard configuration is only available in the desktop app",
  );
}

export async function resetKeyboardConfig(): Promise<KeyboardConfig> {
  return invokeTauriRequired<KeyboardConfig>(
    "reset_keyboard_config",
    undefined,
    "Keyboard configuration is only available in the desktop app",
  );
}
