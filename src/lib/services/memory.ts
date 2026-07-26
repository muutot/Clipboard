import { invoke } from "@tauri-apps/api/core";
import { isTauriRuntime } from "$lib/services/runtime";
import type { MemoryDiagnostics } from "$lib/types/memory";

/**
 * Read a best-effort memory snapshot from the desktop backend.
 *
 * Browser/dev mode has no process-group information, so it intentionally
 * returns `null` instead of fabricating values.
 */
export async function getMemoryDiagnostics(): Promise<MemoryDiagnostics | null> {
  if (!isTauriRuntime()) {
    return null;
  }

  return invoke<MemoryDiagnostics>("get_memory_diagnostics");
}
