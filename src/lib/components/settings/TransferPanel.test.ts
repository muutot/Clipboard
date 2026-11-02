import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale, messages, resolvePath } from "$lib/i18n";
import { get } from "svelte/store";
import Panel from "./StorageToolsPanel.svelte";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  importFile: vi.fn(),
  feedback: vi.fn(),
  adjustLimit: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: bridge.open, save: vi.fn() }));
vi.mock("$lib/services/storage", () => ({
  getImportFormats: vi.fn(async () => [{ id: "json", label: "JSON", extension: ".json" }]),
  getExportFormats: vi.fn(async () => [{ id: "json", label: "JSON", extension: ".json" }]),
  importFromFile: bridge.importFile,
  exportToFile: vi.fn(),
  createResourceBackup: vi.fn(),
  previewResourceBackup: vi.fn(),
  restoreResourceBackup: vi.fn(),
  rebuildSearchIndex: vi.fn(),
  repairDatabase: vi.fn(),
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const summary = (pendingTruncation = 7, maxItems = 42000, errors: string[] = []) => ({
  importedCount: 10,
  skippedCount: 2,
  errors,
  pendingTruncation,
  maxItems,
});
const warning = () => target.querySelector(".transfer-limit-warning");
const translate = (path: string, params?: Record<string, string | number>) =>
  resolvePath(get(messages), path, params);

async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function importButton() {
  return target.querySelector<HTMLButtonElement>(
    '[data-settings-search-id="storage.import-data"] .transfer-group > button',
  )!;
}
async function doImport() {
  importButton().click();
  await vi.waitFor(async () => {
    await settle();
    expect(importButton().disabled).toBe(false);
  });
}
beforeEach(async () => {
  vi.clearAllMocks();
  bridge.invoke
    .mockReset()
    .mockImplementation(async (command) =>
      command === "get_history_config" ? { maxItems: 10000 } : null,
    );
  bridge.open.mockReset().mockResolvedValue("fixture.json");
  bridge.importFile.mockReset().mockResolvedValue(summary());
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onfeedback: bridge.feedback, onadjustlimit: bridge.adjustLimit },
  });
  flushSync();
  await vi.waitFor(async () => {
    await settle();
    expect(importButton().disabled).toBe(false);
  });
});
afterEach(async () => {
  await unmount(component);
  target.remove();
});

it.each(["en", "zh-CN"] as const)(
  "uses the import result's limit in the %s warning and retains the adjustment action",
  async (language) => {
    locale.set(language);
    await doImport();
    expect(bridge.importFile).toHaveBeenCalledWith("fixture.json");
    expect(warning()!.textContent).toContain(
      translate("storage.importTruncationWarning", { max: 42000, count: 7 }),
    );
    warning()!.querySelector<HTMLButtonElement>("button")!.click();
    expect(bridge.adjustLimit).toHaveBeenCalledTimes(1);
  },
);

it("removes a previous warning after a completed import reports no truncation risk", async () => {
  await doImport();
  expect(warning()).not.toBeNull();
  bridge.importFile.mockResolvedValueOnce(summary(0, 60000));
  await doImport();
  expect(bridge.importFile).toHaveBeenCalledTimes(2);
  expect(warning()).toBeNull();
  expect(bridge.feedback).toHaveBeenLastCalledWith(
    translate("storage.importSuccess", { imported: 10, skipped: 2 }),
    true,
  );
});

it("updates both warning counts from a later partial import result", async () => {
  await doImport();
  bridge.importFile.mockResolvedValueOnce(summary(3, 50000, ["fixture row rejected"]));
  await doImport();
  expect(warning()!.textContent).toContain(
    translate("storage.importTruncationWarning", { max: 50000, count: 3 }),
  );
  expect(bridge.feedback).toHaveBeenLastCalledWith(
    expect.stringContaining("fixture row rejected"),
    false,
  );
});

it.each(["cancelled", "rejected"])(
  "retains the last completed import warning when the next attempt is %s",
  async (outcome) => {
    await doImport();
    const previous = warning()!.textContent;
    if (outcome === "cancelled") bridge.open.mockResolvedValueOnce(null);
    else bridge.importFile.mockRejectedValueOnce(new Error("fixture import failure"));
    await doImport();
    expect(warning()!.textContent).toBe(previous);
    expect(bridge.importFile).toHaveBeenCalledTimes(outcome === "cancelled" ? 1 : 2);
    if (outcome === "rejected") {
      expect(bridge.feedback).toHaveBeenLastCalledWith(
        expect.stringContaining("fixture import failure"),
        false,
      );
    }
  },
);
