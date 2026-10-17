import { flushSync, mount, tick, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";

import type { MemoryDiagnostics, MemoryProcess } from "$lib/types/memory";

import StatisticsSettingsPanel from "./StatisticsSettingsPanel.svelte";

const bridge = vi.hoisted(() => ({ memory: vi.fn() }));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));
vi.mock("$lib/services/memory", () => ({
  getMemoryDiagnostics: bridge.memory,
}));

const MiB = 1024 * 1024;

function process(
  pid: number,
  role: string,
  privateWorkingSetBytes: number | null,
  workingSetBytes: number,
): MemoryProcess {
  return {
    pid,
    parentPid: pid === 1 ? null : 1,
    name: `${role}.exe`,
    role,
    workingSetBytes,
    privateBytes: workingSetBytes,
    privateWorkingSetBytes,
    virtualBytes: workingSetBytes,
  };
}

function diagnostics(privateWorkingSetBytes: number | null): MemoryDiagnostics {
  const main = process(1, "main", privateWorkingSetBytes, 512 * MiB);
  return {
    sampledAtMs: 1,
    currentProcess: main,
    processGroup: {
      privateWorkingSetBytes,
      workingSetBytes: 512 * MiB,
      privateBytes: 640 * MiB,
      virtualBytes: 768 * MiB,
      processes: [main],
    },
    system: {
      totalBytes: 32 * 1024 * MiB,
      availableBytes: 12 * 1024 * MiB,
    },
    ocr: {
      engine: "ppocr",
      modelVariant: "small",
      modelBytes: 64 * MiB,
      modelFileCount: 4,
      modelDirectory: "C:\\models\\ppocr-small",
      loaded: true,
      installedVariants: ["small"],
    },
  };
}

const cleanups: (() => Promise<void>)[] = [];

beforeEach(() => {
  bridge.memory.mockReset();
});

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

async function settle() {
  for (let index = 0; index < 10; index++) {
    await new Promise((resolve) => setTimeout(resolve, 0));
    await tick();
    await Promise.resolve();
    flushSync();
  }
}

function render(diagnostics: MemoryDiagnostics) {
  bridge.memory.mockResolvedValueOnce(diagnostics);
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(StatisticsSettingsPanel, {
    target,
    props: {
      activeTab: "memory",
      status: null,
      loading: false,
      onrefreshStatus: async () => {},
      onclose: () => {},
    },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}

it("leads with private resident memory and keeps working set as a diagnostic", async () => {
  const target = render(diagnostics(160 * MiB));
  await settle();
  expect(bridge.memory).toHaveBeenCalledTimes(1);

  const primary = target.querySelector(
    '[data-settings-search-id="statistics.memory.process-group-private-working-set"]',
  );
  const diagnostic = target.querySelector(
    '[data-settings-search-id="statistics.memory.process-group-working-set"]',
  );
  expect(primary?.textContent ?? "").toContain("160 MB");
  expect(diagnostic?.textContent ?? "").toContain("512 MB");

  const processRow = target.querySelector(".memory-process-row:not(.memory-process-header)");
  expect(processRow?.textContent).toContain("160 MB");
  expect(processRow?.textContent).toContain("512 MB");
});

it("falls back to the working set without claiming a private-resident value", async () => {
  const target = render(diagnostics(null));
  await settle();

  const primary = target.querySelector(
    '[data-settings-search-id="statistics.memory.process-group-private-working-set"]',
  );
  expect(primary?.textContent ?? "").toContain("512 MB");
  expect(
    target.querySelector('[data-settings-search-id="statistics.memory.process-group-working-set"]'),
  ).toBeNull();
});
