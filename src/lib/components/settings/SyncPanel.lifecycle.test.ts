import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import Panel from "./SyncPanel.svelte";

const bridge = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  test: vi.fn(),
  run: vi.fn(),
  rotate: vi.fn(),
  feedback: vi.fn(),
  invoke: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("$lib/services/runtime", () => ({
  invokeTauri: bridge.invoke,
  invokeTauriRequired: bridge.invoke,
  isTauriRuntime: () => true,
  getRuntimeInfo: vi.fn(async () => null),
  openDevtools: vi.fn(),
  isDevtoolsHotkey: () => false,
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/storage", () => ({
  getSyncConfig: bridge.get,
  setSyncConfig: bridge.set,
  testSyncConnection: bridge.test,
  runSync: bridge.run,
  rotateSyncNamespace: bridge.rotate,
}));

const config = {
  provider: "s3" as const,
  endpoint: "https://s3.example.com",
  remotePath: "clipboard",
  lastSyncMs: null,
  lastSyncStatus: null,
  pendingEntries: 0,
  autoSync: false,
  autoSyncIntervalSecs: 300,
  segmentMaxEntries: 512,
  resourceGcMode: "off" as const,
  maxSyncImageBytes: 5242880,
  maxSyncFileBytes: 10485760,
  s3Region: "us-east-1",
  s3Bucket: "bucket",
  s3AccessKey: "key",
  hasS3SecretKey: false,
  hasSyncPassword: false,
};

const syncResult = {
  uploadedEntries: 0,
  downloadedEntries: 0,
  appliedEntries: 0,
  failedPeers: 0,
  uploadedResources: 0,
  downloadedResources: 0,
  deletedRemoteObjects: 0,
  bytesUploaded: 0,
  bytesDownloaded: 0,
};

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

async function render(tab: "cloud" | "s3") {
  component = mount(Panel, { target, props: { tab, onfeedback: bridge.feedback } });
  flushSync();
  await vi.waitFor(async () => {
    await settle();
    expect(target.textContent).not.toContain("Reading local configuration");
  });
}

async function teardown() {
  await unmount(component!);
  component = null;
}

beforeEach(() => {
  vi.clearAllMocks();
  bridge.get.mockResolvedValue({ ...config });
  bridge.set.mockResolvedValue(undefined);
  bridge.test.mockResolvedValue({ success: true, message: "connected" });
  bridge.run.mockResolvedValue({ ...syncResult });
  bridge.rotate.mockResolvedValue(undefined);
  bridge.invoke.mockResolvedValue(null);
  locale.set("en");
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  target.remove();
});

it("does not publish an automatic save failure after teardown", async () => {
  let fail!: (error: Error) => void;
  bridge.set.mockReturnValueOnce(new Promise((_, reject) => (fail = reject)));
  await render("cloud");
  target.querySelector<HTMLButtonElement>('button[role="switch"]')!.click();
  await vi.waitFor(() => expect(bridge.set).toHaveBeenCalled());
  await teardown();
  fail(new Error("late save failure"));
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
});

it.each(["succeeded", "failed"] as const)(
  "does not publish a connection test that %s after teardown",
  async (outcome) => {
    let finish!: (value: unknown) => void;
    let fail!: (error: Error) => void;
    bridge.test.mockReturnValueOnce(
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    await render("s3");
    button("Test Connection").click();
    await vi.waitFor(() => expect(bridge.test).toHaveBeenCalled());
    await teardown();
    if (outcome === "failed") fail(new Error("late connection failure"));
    else finish({ success: true, message: "connected" });
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
  },
);

it.each(["succeeded", "failed"] as const)(
  "does not publish a manual sync that %s after teardown",
  async (outcome) => {
    let finish!: (value: unknown) => void;
    let fail!: (error: Error) => void;
    bridge.run.mockReturnValueOnce(
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    await render("cloud");
    button("Sync Now").click();
    await vi.waitFor(() => expect(bridge.run).toHaveBeenCalled());
    await teardown();
    if (outcome === "failed") fail(new Error("late sync failure"));
    else finish({ ...syncResult });
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
  },
);

it.each(["succeeded", "failed"] as const)(
  "does not publish a namespace rotation that %s after teardown",
  async (outcome) => {
    let finish!: () => void;
    let fail!: (error: Error) => void;
    bridge.rotate.mockReturnValueOnce(
      new Promise<void>((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    await render("s3");
    button("Rotate Sync Password").click();
    button("Start Rotation").click();
    await vi.waitFor(() => expect(bridge.rotate).toHaveBeenCalled());
    await teardown();
    if (outcome === "failed") fail(new Error("late rotation failure"));
    else finish();
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
  },
);
