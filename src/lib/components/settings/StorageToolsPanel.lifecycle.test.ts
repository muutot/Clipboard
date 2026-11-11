import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "#lib/i18n/index.ts";
import Panel from "./StorageToolsPanel.svelte";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  importFile: vi.fn(),
  exportFile: vi.fn(),
  feedback: vi.fn(),
  repair: vi.fn(),
  rebuild: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("#lib/services/runtime.ts", () => ({ isTauriRuntime: () => true }));
vi.mock("#lib/services/settings.ts", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: bridge.open, save: bridge.save }));
vi.mock("#lib/services/storage.ts", () => ({
  getImportFormats: vi.fn(async () => [{ id: "json", label: "JSON", extension: ".json" }]),
  getExportFormats: vi.fn(async () => [{ id: "json", label: "JSON", extension: ".json" }]),
  importFromFile: bridge.importFile,
  exportToFile: bridge.exportFile,
  createResourceBackup: vi.fn(),
  previewResourceBackup: vi.fn(),
  restoreResourceBackup: vi.fn(),
  rebuildSearchIndex: bridge.rebuild,
  repairDatabase: bridge.repair,
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

beforeEach(async () => {
  vi.clearAllMocks();
  bridge.invoke.mockImplementation(async (command) =>
    command === "get_history_config" ? { maxItems: 10_000 } : null,
  );
  bridge.open.mockResolvedValue("fixture.json");
  bridge.save.mockResolvedValue("fixture.export.json");
  bridge.importFile.mockResolvedValue({
    importedCount: 0,
    skippedCount: 0,
    errors: [],
    pendingTruncation: 0,
    maxItems: 10_000,
  });
  bridge.exportFile.mockResolvedValue({ path: "fixture.export.json", byteCount: 64 });
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onfeedback: bridge.feedback, onadjustlimit: vi.fn() },
  });
  flushSync();
  await settle();
});

afterEach(async () => {
  await unmount(component);
  target.remove();
});

it("localizes detailed database repair feedback in Chinese", async () => {
  locale.set("zh-CN");
  bridge.repair.mockResolvedValueOnce({
    integrityOk: true,
    integrityMessage: "ok",
    pageCount: 12,
    freelistCount: 3,
  });
  target
    .querySelector<HTMLButtonElement>(
      '[data-settings-search-id="storage.database-maintenance"] button',
    )!
    .click();
  await settle();
  expect(bridge.feedback).toHaveBeenCalledWith("数据库完整性正常（12 页，3 个空闲页）", true);

  bridge.feedback.mockClear();
  bridge.repair.mockRejectedValueOnce(new Error("disk I/O"));
  target
    .querySelector<HTMLButtonElement>(
      '[data-settings-search-id="storage.database-maintenance"] button',
    )!
    .click();
  await settle();
  expect(bridge.feedback).toHaveBeenCalledWith("数据库修复失败：disk I/O", false);
});

it.each([
  ["repair", "succeeded"],
  ["repair", "failed"],
  ["rebuild", "succeeded"],
  ["rebuild", "failed"],
] as const)("does not publish a %s that %s after teardown", async (operation, outcome) => {
  let finish!: (value: unknown) => void;
  let fail!: (error: Error) => void;
  const pending = new Promise((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  (operation === "repair" ? bridge.repair : bridge.rebuild).mockReturnValue(pending);
  const button = target.querySelector<HTMLButtonElement>(
    operation === "repair"
      ? '[data-settings-search-id="storage.database-maintenance"] button'
      : '[data-settings-search-id="storage.search-index"] button',
  )!;
  button.click();
  await settle();
  await unmount(component);
  if (outcome === "failed") fail(new Error("late fixture failure"));
  else if (operation === "repair") {
    finish({ integrityOk: true, integrityMessage: "ok", pageCount: 1, freelistCount: 0 });
  } else {
    finish({ processedEvents: 1, upsertedDocuments: 1, deletedDocuments: 0, lastSequence: 1 });
  }
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
  target.remove();
});

it.each([
  ["import", "succeeded"],
  ["import", "failed"],
  ["export", "succeeded"],
  ["export", "failed"],
] as const)("does not publish an %s that %s after teardown", async (operation, outcome) => {
  let finish!: (value: unknown) => void;
  let fail!: (error: Error) => void;
  const pending = new Promise((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  const service = operation === "import" ? bridge.importFile : bridge.exportFile;
  service.mockReturnValueOnce(pending);
  const actionButton = target.querySelector<HTMLButtonElement>(
    `[data-settings-search-id="storage.${operation}-data"] .transfer-group > button`,
  )!;
  await vi.waitFor(async () => {
    await settle();
    expect(actionButton.disabled).toBe(false);
  });
  actionButton.click();
  await vi.waitFor(() => expect(service).toHaveBeenCalled());

  await unmount(component);
  if (outcome === "failed") fail(new Error("late operation failure"));
  else if (operation === "import") {
    finish({
      importedCount: 1,
      skippedCount: 0,
      errors: [],
      pendingTruncation: 0,
      maxItems: 10_000,
    });
  } else {
    finish({ path: "fixture.export.json", byteCount: 64 });
  }
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
  target.remove();
});
