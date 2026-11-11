import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import type { StorageStatus } from "#lib/services/storage.ts";
import { locale } from "#lib/i18n/index.ts";
import { confirmDialog, currentConfirm, settleConfirm } from "#lib/services/ui/confirm.svelte.ts";
import Panel from "./StoragePathsPanel.svelte";
import ConfirmHost from "../common/ConfirmDialog.svelte";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), feedback: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("#lib/services/settings.ts", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});

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
  usesCustomDataDirectory: true,
  storagePath: "C:/fixture/storage",
  iconsDir: "C:/fixture/storage/icons",
  databasePath: "C:/fixture/storage/db",
  databaseSizeBytes: 0,
  filesPath: "C:/fixture/files",
  imagePath: "C:/fixture/images",
  imageCleanupEnabled: false,
  fileCleanupEnabled: false,
  resourceOwnershipRequired: true,
  resourceOwned: false,
  searchIndexPath: "C:/fixture/index",
  searchIndexSizeBytes: 0,
  searchIndexVersion: 1,
  searchIndexRebuildRequired: false,
  diskTotalBytes: null,
  diskAvailableBytes: null,
};
function response(command: string) {
  if (command === "get_storage_config")
    return {
      maxFileCopySizeBytes: 1,
      maxScreenshotSizeBytes: 1,
      imageStoragePath: null,
      fileStoragePath: null,
    };
  if (command === "set_resource_ownership") return { restartRequired: true };
  throw new Error(`Unexpected fixture command: ${command}`);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
let target: HTMLDivElement;
let hostTarget: HTMLDivElement;
let panel: ReturnType<typeof mount> | undefined;
let host: ReturnType<typeof mount>;
async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
async function closePanel() {
  await unmount(panel!);
  panel = undefined;
}
const writes = () => bridge.invoke.mock.calls.filter(([cmd]) => cmd === "set_resource_ownership");
const toggle = () => target.querySelector<HTMLButtonElement>(".ownership-controls .toggle-switch")!;
const accept = () => hostTarget.querySelector<HTMLButtonElement>(".confirm-accept")!.click();
const cancel = () => hostTarget.querySelector<HTMLButtonElement>(".confirm-cancel")!.click();
beforeAll(() => {
  // Keep the real confirmation singleton and host; jsdom lacks the modal API.
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
});
beforeEach(async () => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  bridge.invoke.mockReset().mockImplementation(async (command) => response(command));
  bridge.feedback.mockReset();
  settleConfirm(false);
  locale.set("en");
  target = document.createElement("div");
  hostTarget = document.createElement("div");
  document.body.append(target, hostTarget);
  host = mount(ConfirmHost, { target: hostTarget });
  panel = mount(Panel, { target, props: { status, onfeedback: bridge.feedback } });
  flushSync();
  await settle();
});
afterEach(async () => {
  settleConfirm(false);
  await settle();
  if (panel) await closePanel();
  await unmount(host);
  target.remove();
  hostTarget.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("cancels its ownership confirmation when its panel is destroyed", async () => {
  toggle().click();
  await settle();
  expect(currentConfirm()?.message).toContain("cleanup deletes files");
  await closePanel();
  await settle();
  expect(currentConfirm()).toBeNull();
  expect(hostTarget.querySelector(".confirm-dialog")).toBeNull();
  expect(writes()).toHaveLength(0);
});

it("does not submit ownership when teardown precedes the acceptance continuation", async () => {
  toggle().click();
  await settle();
  accept();
  await closePanel();
  await settle();
  expect(writes()).toHaveLength(0);
  expect(bridge.feedback).not.toHaveBeenCalled();
});

it("keeps another caller's newer confirmation when its panel is destroyed", async () => {
  toggle().click();
  await settle();
  const newer = confirmDialog({ title: "Newer request", message: "Keep this current" });
  await closePanel();
  await settle();
  expect(hostTarget.querySelector(".confirm-title")!.textContent).toBe("Newer request");
  expect(writes()).toHaveLength(0);
  settleConfirm(false);
  await expect(newer).resolves.toBe(false);
});

it.each(["succeeded", "failed"])(
  "does not publish an ownership write that %s after panel teardown",
  async (outcome) => {
    const pending = deferred<{ restartRequired: boolean }>();
    bridge.invoke.mockImplementation((command) =>
      command === "set_resource_ownership" ? pending.promise : Promise.resolve(response(command)),
    );
    toggle().click();
    await settle();
    accept();
    await settle();
    expect(writes()).toEqual([["set_resource_ownership", { owned: true }]]);
    await closePanel();
    if (outcome === "succeeded") pending.resolve({ restartRequired: true });
    else pending.reject(new Error("late fixture ownership failure"));
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
  },
);

it("keeps cancelled enable retryable, saves accepted enable and disables without confirmation", async () => {
  toggle().click();
  await settle();
  cancel();
  await settle();
  expect(writes()).toHaveLength(0);
  expect(toggle().disabled).toBe(false);
  toggle().click();
  await settle();
  accept();
  await settle();
  expect(toggle().getAttribute("aria-checked")).toBe("true");
  expect(bridge.feedback).toHaveBeenLastCalledWith(expect.any(String), true);
  toggle().click();
  await settle();
  expect(writes()).toEqual([
    ["set_resource_ownership", { owned: true }],
    ["set_resource_ownership", { owned: false }],
  ]);
  expect(currentConfirm()).toBeNull();
  expect(toggle().getAttribute("aria-checked")).toBe("false");
});

it("retains the confirmed state after a mounted write failure and permits retry", async () => {
  bridge.invoke.mockImplementation(async (command) => {
    if (command === "set_resource_ownership") throw new Error("fixture ownership failure");
    return response(command);
  });
  toggle().click();
  await settle();
  accept();
  await settle();
  expect(toggle().getAttribute("aria-checked")).toBe("false");
  expect(toggle().disabled).toBe(false);
  expect(bridge.feedback).toHaveBeenCalledWith("fixture ownership failure", false);
  bridge.invoke.mockImplementation(async (command) => response(command));
  toggle().click();
  await settle();
  accept();
  await settle();
  expect(toggle().getAttribute("aria-checked")).toBe("true");
});
