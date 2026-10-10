export const FILE_PREVIEW_LIMIT = 512 * 1024;

/** Request only a prefix and bound retained bytes even if Range is ignored. */
export async function loadFilePreview(url: string, signal: AbortSignal) {
  const response = await fetch(url, {
    headers: { Range: `bytes=0-${FILE_PREVIEW_LIMIT}` },
    signal,
  });
  signal.throwIfAborted();
  // Tauri's asset protocol returns 416 for a range against an empty file.
  if (response.status === 416 && response.headers.get("Content-Range") === "bytes */0") {
    return { text: "", truncated: false };
  }
  if (!response.ok) throw new Error(`HTTP ${response.status}`);
  if (!response.body) return { text: "", truncated: false };
  const reader = response.body.getReader();
  const bytes = new Uint8Array(FILE_PREVIEW_LIMIT + 1);
  let used = 0;
  const cancel = () => {
    void reader.cancel().catch(() => {});
  };
  signal.addEventListener("abort", cancel, { once: true });
  try {
    while (used < bytes.length) {
      signal.throwIfAborted();
      const { done, value } = await reader.read();
      signal.throwIfAborted();
      if (done) break;
      const count = Math.min(value.length, bytes.length - used);
      bytes.set(value.subarray(0, count), used);
      used += count;
    }
    return {
      text: new TextDecoder("utf-8").decode(bytes.subarray(0, Math.min(used, FILE_PREVIEW_LIMIT))),
      truncated: used > FILE_PREVIEW_LIMIT,
    };
  } finally {
    signal.removeEventListener("abort", cancel);
    await reader.cancel().catch(() => {});
    reader.releaseLock();
  }
}
