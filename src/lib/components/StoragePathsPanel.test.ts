import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { StorageStatus } from "$lib/services/storage";
import StoragePathsPanel from "./StoragePathsPanel.svelte";

const bridge = vi.hoisted(() => ({ load: vi.fn(), save: vi.fn(), feedback: vi.fn() }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/storage", () => ({
  getStorageConfig: bridge.load,
  setResourceStoragePaths: bridge.save,
  configureStorageDirectory: vi.fn(),
  setResourceOwnership: vi.fn(),
}));
const configured = {
  maxFileCopySizeBytes: 1,
  maxScreenshotSizeBytes: 1,
  imageStoragePath: "C:/next/images",
  fileStoragePath: "C:/next/files",
};
const status: StorageStatus = {
  itemCount: 0,
  imageCount: 0,
  imageSizeBytes: 0,
  fileCount: 0,
  fileSizeBytes: 0,
  textCount: 0,
  linkCount: 0,
  projectPath: "C:/fixture",
  configPath: "C:/fixture/conf",
  keyboardConfigPath: "C:/fixture/keyboard",
  dataDirectoryPath: "C:/fixture",
  usesCustomDataDirectory: false,
  storagePath: "C:/fixture/storage",
  iconsDir: "C:/fixture/storage/icons",
  databasePath: "C:/fixture/storage/db",
  databaseSizeBytes: 0,
  filesPath: "C:/active/files",
  imagePath: "C:/active/images",
  imageCleanupEnabled: true,
  fileCleanupEnabled: true,
  resourceOwnershipRequired: false,
  resourceOwned: true,
  searchIndexPath: "C:/fixture/index",
  searchIndexSizeBytes: 0,
  searchIndexVersion: 1,
  searchIndexRebuildRequired: false,
  diskTotalBytes: null,
  diskAvailableBytes: null,
};
const cleanups: (() => Promise<void>)[] = [];
beforeEach(() => {
  bridge.load.mockReset().mockResolvedValue(configured);
  bridge.save.mockReset().mockImplementation(async (image, file) => ({
    imageStoragePath: image ?? "default-image",
    fileStoragePath: file ?? "default-file",
    restartRequired: true,
  }));
  bridge.feedback.mockReset();
});
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});
async function settle() {
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function render() {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(StoragePathsPanel, { target, props: { status, onfeedback: bridge.feedback } });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}
it("preserves the unedited persisted path, including a pending restart", async () => {
  const target = render();
  await settle();
  const image = target.querySelector<HTMLInputElement>("#image-storage-path")!;
  const file = target.querySelector<HTMLInputElement>("#file-storage-path")!;
  expect(image.value).toBe(configured.imageStoragePath);
  expect(file.value).toBe(configured.fileStoragePath);
  image.value = "C:/changed/images";
  image.dispatchEvent(new Event("input", { bubbles: true }));
  target.querySelectorAll<HTMLButtonElement>(".resource-path-actions button")[1].click();
  await settle();
  expect(bridge.save).toHaveBeenCalledWith("C:/changed/images", configured.fileStoragePath);
});
it("does not render blank editable paths while configuration is pending or failed", async () => {
  let reject!: (error: Error) => void;
  bridge.load.mockReturnValue(
    new Promise((_, fail) => {
      reject = fail;
    }),
  );
  const target = render();
  expect(target.querySelector("#image-storage-path")).toBeNull();
  reject(new Error("fixture failure"));
  await settle();
  expect(target.querySelector("#image-storage-path")).toBeNull();
  expect(bridge.feedback).toHaveBeenCalledWith("fixture failure", false);
  expect(bridge.save).not.toHaveBeenCalled();
});
it("restores defaults explicitly and retains the draft if saving fails", async () => {
  const target = render();
  await settle();
  bridge.save.mockRejectedValueOnce(new Error("fixture save failure"));
  target.querySelectorAll<HTMLButtonElement>(".resource-path-actions button")[0].click();
  await settle();
  expect(bridge.save).toHaveBeenCalledWith(null, null);
  expect(target.querySelector<HTMLInputElement>("#file-storage-path")!.value).toBe(
    configured.fileStoragePath,
  );
  target.querySelectorAll<HTMLButtonElement>(".resource-path-actions button")[0].click();
  await settle();
  expect(target.querySelector<HTMLInputElement>("#image-storage-path")!.value).toBe("");
  expect(target.querySelector<HTMLInputElement>("#file-storage-path")!.value).toBe("");
});
