import { invoke } from "@tauri-apps/api/core";
import type { RuntimeInfo } from "$lib/types/clipboard";

export async function getRuntimeInfo(): Promise<RuntimeInfo | null> {
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    return null;
  }

  try {
    return await invoke<RuntimeInfo>("get_runtime_info");
  } catch (error) {
    console.warn("Unable to read Tauri runtime information", error);
    return null;
  }
}
