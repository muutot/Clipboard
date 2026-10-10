import { it, expect } from "vitest";
import { createCardMeasurementCache } from "./card-measurements";
import type { ClipboardItem } from "$lib/types/clipboard";

const runtime = globalThis as unknown as {
  process: { env: Record<string, string | undefined>; stdout: { write(value: string): void } };
};

// Opt-in, synthetic CPU/serialization benchmark. No wall-clock pass threshold.
// PowerShell: $env:CLIPBOARD_BENCHMARK='1'; npm test -- history-performance
it.skipIf(runtime.process.env.CLIPBOARD_BENCHMARK !== "1")(
  "reports repeatable synthetic history costs",
  () => {
    const rows = Array.from({ length: 1000 }, (_, i) => ({
      id: `synthetic-${i}`,
      kind: "text",
      title: `Record ${i}`,
      textContent: (`record ${i}\r\n` + "x".repeat(110) + "\r\n").repeat(256),
      html: "<p>" + "r".repeat(32_768) + "</p>",
    })) as unknown as ClipboardItem[];
    let sink = 0;
    const median = (run: () => void) => {
      for (let i = 0; i < 3; i++) run();
      const samples = Array.from({ length: 9 }, () => {
        const start = performance.now();
        run();
        return performance.now() - start;
      });
      return samples.sort((a, b) => a - b)[4];
    };
    const cache = createCardMeasurementCache();
    const oldScanMs = median(() => {
      for (const row of rows) sink += row.textContent!.replace(/\r\n?/g, "\n").split("\n").length;
    });
    const tokenScanMs = median(() => {
      for (const row of rows) sink += cache.token(row);
    });
    const summary = rows.map((row) => ({
      ...row,
      textContent: row.textContent!.slice(0, 2048),
      html: "1",
    }));
    const fullBytes = new TextEncoder().encode(JSON.stringify(rows)).byteLength;
    const summaryBytes = new TextEncoder().encode(JSON.stringify(summary)).byteLength;
    const fullSerializeMs = median(() => {
      sink += JSON.stringify(rows).length;
    });
    const summarySerializeMs = median(() => {
      sink += JSON.stringify(summary).length;
    });
    expect(sink).toBeGreaterThan(0);
    expect(summaryBytes).toBeLessThan(fullBytes);
    runtime.process.stdout.write(
      JSON.stringify(
        {
          rows: rows.length,
          warmups: 3,
          samples: 9,
          oldScanMs,
          tokenScanMs,
          fullBytes,
          summaryBytes,
          fullSerializeMs,
          summarySerializeMs,
        },
        null,
        2,
      ) + "\n",
    );
  },
);
