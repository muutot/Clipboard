import { mount, unmount, flushSync } from "svelte";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import * as storage from "$lib/services/storage";
import Panel from "./BackupPanel.svelte";
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/storage", () => ({
  createResourceBackup: vi.fn(),
  previewResourceBackup: vi.fn(),
  restoreResourceBackup: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(async () => "test.clipbackup"),
  save: vi.fn(async () => "test.clipbackup"),
}));
let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const feedback = vi.fn();
const button = (name: string) =>
  [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === name);
async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
beforeEach(() => {
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  vi.mocked(storage.previewResourceBackup).mockResolvedValue({
    fingerprint: "sha256",
    itemCount: 3,
    duplicateCount: 1,
    resourceCount: 2,
    resourceBytes: 100,
  });
  vi.mocked(storage.restoreResourceBackup).mockResolvedValue({
    importedCount: 2,
    skippedCount: 1,
    errors: [],
    pendingTruncation: 2,
    maxItems: 100,
  });
  component = mount(Panel, { target, props: { onfeedback: feedback, onadjustlimit: vi.fn() } });
  flushSync();
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  vi.clearAllMocks();
});
it("requires a validated preview and sends its fingerprint on restore", async () => {
  expect(button("Restore validated backup")).toBeUndefined();
  button("Select and validate backup")!.click();
  await vi.waitFor(async () => {
    await settle();
    expect(target.textContent).toContain("3 records (1 already present)");
  });
  button("Restore validated backup")!.click();
  await vi.waitFor(async () => {
    await settle();
    expect(storage.restoreResourceBackup).toHaveBeenCalledWith("test.clipbackup", "sha256");
  });
  expect(button("Restore validated backup")).toBeUndefined();
  expect(target.textContent).toContain("2 restored records");
});
it("does not expose restore after validation failure", async () => {
  vi.mocked(storage.previewResourceBackup).mockRejectedValueOnce(new Error("checksum mismatch"));
  button("Select and validate backup")!.click();
  await vi.waitFor(async () => {
    await settle();
    expect(feedback).toHaveBeenCalledWith("checksum mismatch", false);
  });
  expect(button("Restore validated backup")).toBeUndefined();
  expect(storage.restoreResourceBackup).not.toHaveBeenCalled();
});
