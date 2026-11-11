import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "#lib/i18n/index.ts";
import Panel from "./BackupPanel.svelte";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  open: vi.fn(),
  save: vi.fn(),
  create: vi.fn(),
  preview: vi.fn(),
  restore: vi.fn(),
  feedback: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: bridge.open, save: bridge.save }));
vi.mock("#lib/services/runtime.ts", () => ({
  invokeTauri: bridge.invoke,
  invokeTauriRequired: bridge.invoke,
  isTauriRuntime: () => true,
  getRuntimeInfo: vi.fn(async () => null),
  openDevtools: vi.fn(),
  isDevtoolsHotkey: () => false,
}));
vi.mock("#lib/services/settings.ts", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("#lib/services/storage.ts", () => ({
  createResourceBackup: bridge.create,
  previewResourceBackup: bridge.preview,
  restoreResourceBackup: bridge.restore,
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | null;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

function button(label: string) {
  return [...target.querySelectorAll<HTMLButtonElement>("button")].find(
    (entry) => entry.textContent?.trim() === label,
  )!;
}

beforeEach(() => {
  vi.clearAllMocks();
  bridge.invoke.mockResolvedValue(null);
  bridge.open.mockResolvedValue("fixture.clipbackup");
  bridge.save.mockResolvedValue("fixture.clipbackup");
  bridge.create.mockResolvedValue({ path: "fixture.clipbackup", byteCount: 64 });
  bridge.preview.mockResolvedValue({
    fingerprint: "sha256",
    itemCount: 3,
    duplicateCount: 1,
    resourceCount: 2,
    resourceBytes: 100,
  });
  bridge.restore.mockResolvedValue({
    importedCount: 2,
    skippedCount: 1,
    errors: [],
    pendingTruncation: 0,
    maxItems: 100,
  });
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onfeedback: bridge.feedback, onadjustlimit: vi.fn() },
  });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  target.remove();
});

it.each([
  ["create", "succeeded"],
  ["create", "failed"],
  ["restore", "succeeded"],
  ["restore", "failed"],
] as const)("does not publish a %s that %s after teardown", async (operation, outcome) => {
  if (operation === "restore") {
    button("Select and validate backup").click();
    await vi.waitFor(async () => {
      await settle();
      expect(button("Restore validated backup")).toBeTruthy();
    });
  }

  let finish!: (value: unknown) => void;
  let fail!: (error: Error) => void;
  const pending = new Promise((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  const service = operation === "create" ? bridge.create : bridge.restore;
  service.mockReturnValueOnce(pending);
  button(operation === "create" ? "Create backup" : "Restore validated backup").click();
  await vi.waitFor(() => expect(service).toHaveBeenCalled());

  await unmount(component!);
  component = null;
  if (outcome === "failed") fail(new Error("late operation failure"));
  else if (operation === "create") finish({ path: "fixture.clipbackup", byteCount: 64 });
  else {
    finish({
      importedCount: 2,
      skippedCount: 1,
      errors: [],
      pendingTruncation: 0,
      maxItems: 100,
    });
  }
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
});
