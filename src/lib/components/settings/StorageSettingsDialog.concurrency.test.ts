import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { t } from "$lib/i18n";
import type { StorageStatus } from "$lib/services/storage";
import StorageSettingsDialog from "./StorageSettingsDialog.svelte";

const bridge = vi.hoisted(() => ({
  getStorageStatus: vi.fn(),
  listeners: new Map<string, (event: unknown) => void>(),
}));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "settings" }),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn((event: string, listener: (event: unknown) => void) => {
    bridge.listeners.set(event, listener);
    return Promise.resolve(() => bridge.listeners.delete(event));
  }),
}));
vi.mock("$lib/services/storage", () => ({
  getStorageStatus: bridge.getStorageStatus,
  getPerformanceMetrics: vi.fn().mockResolvedValue(null),
}));

const cleanups: (() => Promise<void>)[] = [];

afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  bridge.getStorageStatus.mockReset();
  bridge.listeners.clear();
});

function status(itemCount: number): StorageStatus {
  return {
    itemCount,
    imageCount: 0,
    imageSizeBytes: 0,
    fileCount: 0,
    fileSizeBytes: 0,
    textCount: itemCount,
    linkCount: 0,
    projectPath: "",
    configPath: "",
    keyboardConfigPath: "",
    dataDirectoryPath: "",
    usesCustomDataDirectory: false,
    storagePath: "",
    iconsDir: "",
    databasePath: "",
    databaseSizeBytes: 0,
    filesPath: "",
    imagePath: "",
    imageCleanupEnabled: false,
    fileCleanupEnabled: false,
    resourceOwnershipRequired: false,
    resourceOwned: true,
    searchIndexPath: "",
    searchIndexSizeBytes: 0,
    searchIndexVersion: 1,
    searchIndexRebuildRequired: false,
    diskTotalBytes: null,
    diskAvailableBytes: null,
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((complete) => {
    resolve = complete;
  });
  return { promise, resolve };
}

async function settle(rounds = 20): Promise<void> {
  for (let index = 0; index < rounds; index += 1) {
    await Promise.resolve();
    flushSync();
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function waitFor<T>(find: () => T | null, timeoutMs = 2000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const found = find();
    if (found) return found;
    if (Date.now() >= deadline) throw new Error("element did not appear");
    await settle(1);
  }
}

describe("StorageSettingsDialog storage statistics ordering", () => {
  it("keeps the newest storage snapshot when an earlier refresh finishes later", async () => {
    const olderRefresh = deferred<StorageStatus>();
    const newestRefresh = deferred<StorageStatus>();
    const pending = new Promise<StorageStatus>(() => {});
    bridge.getStorageStatus
      .mockReturnValue(pending)
      .mockResolvedValueOnce(status(1))
      .mockReturnValueOnce(olderRefresh.promise)
      .mockReturnValueOnce(newestRefresh.promise);

    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(StorageSettingsDialog, {
      target,
      props: { open: true, onclose: () => {} },
    });
    cleanups.push(async () => {
      await unmount(app);
      target.remove();
    });

    const statisticsButton = await waitFor(
      () =>
        Array.from(target.querySelectorAll<HTMLButtonElement>(".settings-primary-nav button")).find(
          (button) => button.textContent?.trim() === t("storage.statisticsTab"),
        ) ?? null,
    );
    statisticsButton.click();

    const metric = await waitFor(() => target.querySelector<HTMLElement>(".stats-metric-value"));
    expect(metric.textContent?.trim()).toBe("1");
    await waitFor(() => bridge.getStorageStatus.mock.calls.length >= 2 || null);

    const notify = bridge.listeners.get("clipboard-item-added");
    expect(notify).toBeDefined();
    notify?.(null);
    await new Promise((resolve) => setTimeout(resolve, 300));
    await waitFor(() => bridge.getStorageStatus.mock.calls.length >= 3 || null);

    newestRefresh.resolve(status(3));
    await settle();
    expect(metric.textContent?.trim()).toBe("3");

    olderRefresh.resolve(status(2));
    await settle();
    expect(target.querySelector<HTMLElement>(".stats-metric-value")?.textContent?.trim()).toBe("3");
  });
});
