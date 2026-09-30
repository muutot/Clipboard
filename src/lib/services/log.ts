import { invokeTauri } from "./runtime";

/**
 * Mirrors one frontend diagnostic line into the application log.
 *
 * The webview console is invisible unless the user opens developer tools, so a
 * failure that only produced a toast could not be diagnosed from the shipped log
 * or a bug report — this is the channel that fixes that.
 *
 * Redaction is the caller's responsibility and follows the same rule as the rest
 * of the logging: ids, paths, counts, config keys and error text only, never
 * clipboard content. Passing a record's `textContent`/`preview`/`searchableText`
 * here would leak clipboard history into a file on disk.
 */
export function logFrontendMessage(level: "error" | "warn" | "info", message: string): void {
  void invokeTauri("log_frontend_message", { level, message }).catch(() => {
    // Diagnostics must never break the operation that produced them.
  });
}

/** Logs a caught value at error level, then re-throws nothing. */
export function logFrontendError(scope: string, error: unknown): void {
  const detail = error instanceof Error ? error.message : String(error);
  logFrontendMessage("error", `${scope}: ${detail}`);
}
