import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { StorageStatus } from "#lib/services/storage.ts";
import StoragePathsPanel from "./StoragePathsPanel.svelte";

const bridge = vi.hoisted(() => ({
  load: vi.fn(),
  save: vi.fn(),
  directory: vi.fn(),
  feedback: vi.fn(),
}));
vi.mock("#lib/services/settings.ts", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("#lib/services/storage.ts", () => ({
  getStorageConfig: bridge.load,
  setResourceStoragePaths: bridge.save,
  configureStorageDirectory: bridge.directory,
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
  bridge.directory.mockReset();
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

it.each(["save", "restore"])(
  "preserves a later directory draft when a pending %s returns",
  async (operation) => {
    let finish!: (result: {
      dataDirectoryPath: string;
      storagePath: string;
      restartRequired: boolean;
    }) => void;
    bridge.directory.mockReturnValue(
      new Promise((resolve) => {
        finish = resolve;
      }),
    );
    const target = render();
    await settle();
    const input = target.querySelector<HTMLInputElement>("#data-directory")!;
    input.value = "C:/submitted";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    const actions = input.parentElement!.querySelectorAll<HTMLButtonElement>("button");
    actions[operation === "save" ? 1 : 0].click();
    await settle();
    expect(bridge.directory).toHaveBeenCalledWith(operation === "save" ? "C:/submitted" : null);
    expect(actions[1].disabled).toBe(true);
    expect(input.disabled).toBe(false);
    // The directory field remains editable while the submitted request is pending.
    input.value = "C:/later-draft";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    finish({
      dataDirectoryPath: "C:/resolved",
      storagePath: "C:/resolved/storage",
      restartRequired: true,
    });
    await settle();
    expect(input.value).toBe("C:/later-draft");
    expect(target.querySelector(".pending-path code")!.textContent).toBe("C:/resolved/storage");
    expect(actions[1].disabled).toBe(false);
    expect(bridge.feedback).toHaveBeenCalledWith(expect.any(String), true);
    actions[1].click();
    await settle();
    expect(bridge.directory).toHaveBeenLastCalledWith("C:/later-draft");
  },
);

it.each([
  ["directory", "succeeded"],
  ["directory", "failed"],
  ["resource", "succeeded"],
  ["resource", "failed"],
])("does not publish a %s save that %s after teardown", async (operation, outcome) => {
  let finish!: (result: unknown) => void;
  let fail!: (error: Error) => void;
  const pending = new Promise((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  (operation === "directory" ? bridge.directory : bridge.save).mockReturnValue(pending);
  const target = render();
  await settle();
  const actions =
    operation === "directory"
      ? target.querySelector("#data-directory")!.parentElement!.querySelectorAll("button")
      : target.querySelectorAll(".resource-path-actions button");
  (actions[1] as HTMLButtonElement).click();
  await settle();
  expect(operation === "directory" ? bridge.directory : bridge.save).toHaveBeenCalledTimes(1);
  await cleanups.pop()!();
  if (outcome === "failed") fail(new Error("late fixture path failure"));
  else
    finish(
      operation === "directory"
        ? {
            dataDirectoryPath: "C:/saved",
            storagePath: "C:/saved/storage",
            restartRequired: true,
          }
        : {
            imageStoragePath: "C:/saved/images",
            fileStoragePath: "C:/saved/files",
            restartRequired: true,
          },
    );
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
});

it.each(["save", "restore"])(
  "applies the resolved directory after %s when the submitted draft is unchanged",
  async (operation) => {
    bridge.directory.mockResolvedValue({
      dataDirectoryPath: "C:/resolved",
      storagePath: "C:/resolved/storage",
      restartRequired: true,
    });
    const target = render();
    await settle();
    const input = target.querySelector<HTMLInputElement>("#data-directory")!;
    input.value = " C:/submitted ";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    flushSync();
    input
      .parentElement!.querySelectorAll<HTMLButtonElement>("button")
      [operation === "save" ? 1 : 0].click();
    await settle();
    expect(bridge.directory).toHaveBeenCalledWith(operation === "save" ? "C:/submitted" : null);
    expect(input.value).toBe("C:/resolved");
  },
);
