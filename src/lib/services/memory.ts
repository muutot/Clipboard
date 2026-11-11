import { invokeTauri } from "#lib/services/runtime.ts";
import type { MemoryDiagnostics } from "#lib/types/memory.ts";

/**
 * Read a best-effort memory snapshot from the desktop backend.
 *
 * Browser/dev mode has no process-group information, so it intentionally
 * returns `null` instead of fabricating values.
 */
export async function getMemoryDiagnostics(): Promise<MemoryDiagnostics | null> {
  return invokeTauri<MemoryDiagnostics>("get_memory_diagnostics");
}
