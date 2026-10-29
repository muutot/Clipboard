import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import * as storage from "$lib/services/storage";
import SyncPanel from "./SyncPanel.svelte";

// One bridge keeps the storage mock hoisted above the vi.mock factories.
const bridge = vi.hoisted(() => ({
  getConfig: vi.fn(),
  setConfig: vi.fn(),
  testConnection: vi.fn(),
  runSync: vi.fn(),
  rotate: vi.fn(),
  invoke: vi.fn(async (command: string) =>
    command === "get_background_operation" ? null : undefined,
  ),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/storage", () => ({
  getSyncConfig: bridge.getConfig,
  setSyncConfig: bridge.setConfig,
  testSyncConnection: bridge.testConnection,
  runSync: bridge.runSync,
  rotateSyncNamespace: bridge.rotate,
}));

const syncConfig = {
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

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const feedback = vi.fn();

const button = (text: string) =>
  [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;
const dialog = () => target.querySelector("dialog")!;

async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}

beforeEach(() => {
  locale.set("en");
  bridge.getConfig.mockReset().mockResolvedValue({ ...syncConfig });
  bridge.setConfig.mockReset().mockResolvedValue(undefined);
  bridge.testConnection.mockReset().mockResolvedValue({ success: true, message: "ok" });
  bridge.runSync.mockReset();
  bridge.rotate.mockReset().mockResolvedValue(undefined);
  feedback.mockReset();
  // jsdom has no modal dialog plumbing; the repo stubs the two methods.
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
  await unmount(component);
  target.remove();
  vi.clearAllMocks();
});

function render(tab: "cloud" | "advanced" | "s3" = "s3") {
  component = mount(SyncPanel, { target, props: { tab, onfeedback: feedback } });
  flushSync();
}

it("persists the resource reference mode and explains that bodies are retained", async () => {
  render("advanced");
  await settle();

  const trigger = target.querySelector<HTMLButtonElement>(
    `button[aria-label='Remote Resource References']`,
  )!;
  trigger.click();
  flushSync();
  const option = [...target.querySelectorAll<HTMLButtonElement>("[role='option']")].find(
    (candidate) => candidate.textContent?.trim() === "Manifest (faster)",
  )!;
  option.click();
  await settle();

  expect(bridge.setConfig).toHaveBeenCalledWith(
    expect.objectContaining({ resourceGcMode: "manifest" }),
  );
  expect(target.textContent).toContain("unused resource files are kept");
});

it("blocks the form until the stored configuration arrives", async () => {
  let resolveConfig!: (value: typeof syncConfig) => void;
  bridge.getConfig.mockReturnValue(
    new Promise((resolve) => {
      resolveConfig = resolve;
    }),
  );
  render();
  expect(target.querySelector(".settings-state")).not.toBeNull();
  expect(target.textContent).toContain("Reading local configuration");
  resolveConfig(syncConfig);
  await settle();
  expect(target.querySelector(".settings-state")).toBeNull();
  const endpoint = target.querySelector<HTMLInputElement>("input[aria-label='Service Endpoint']")!;
  expect(endpoint.value).toBe("https://s3.example.com");
  expect(bridge.setConfig).not.toHaveBeenCalled();
});

it("blocks editing when the configuration load fails", async () => {
  bridge.getConfig.mockRejectedValue(new Error("config locked"));
  render();
  await settle();
  expect(target.querySelector(".settings-state")).not.toBeNull();
  expect(target.querySelector("input[aria-label='Service Endpoint']")).toBeNull();
  expect(bridge.setConfig).not.toHaveBeenCalled();
  expect(bridge.rotate).not.toHaveBeenCalled();
});

it("rotates the namespace with the entered password and reloads the configuration", async () => {
  render();
  await settle();
  button("Rotate Sync Password").click();
  expect(dialog().hasAttribute("open")).toBe(true);

  const password = dialog().querySelector<HTMLInputElement>("input[type=password]")!;
  password.value = "brand-new-password";
  password.dispatchEvent(new Event("input", { bubbles: true }));
  button("Start Rotation").click();
  await settle();

  expect(bridge.rotate).toHaveBeenCalledWith("brand-new-password");
  expect(feedback).toHaveBeenCalledWith("Namespace rotated", true);
  // The backend rewrote the prefix and password, so the panel re-reads them.
  expect(bridge.getConfig).toHaveBeenCalledTimes(2);
  expect(dialog().hasAttribute("open")).toBe(false);
});

it("sends an empty password when encryption is turned off", async () => {
  render();
  await settle();
  button("Rotate Sync Password").click();
  button("Start Rotation").click();
  await settle();
  expect(bridge.rotate).toHaveBeenCalledWith("");
  expect(dialog().hasAttribute("open")).toBe(false);
});

it("keeps the dialog open and reports the reason when rotation fails", async () => {
  bridge.rotate.mockRejectedValueOnce(new Error("another device already rotated this prefix"));
  render();
  await settle();
  button("Rotate Sync Password").click();
  button("Start Rotation").click();
  await settle();
  expect(feedback).toHaveBeenCalledWith("Error: another device already rotated this prefix", false);
  expect(dialog().hasAttribute("open")).toBe(true);
  // The local configuration still points at the old prefix, so it must not
  // have been rewritten.
  expect(bridge.setConfig).not.toHaveBeenCalled();
});

it("hides the rotation section while sync is off", async () => {
  bridge.getConfig.mockResolvedValue({ ...syncConfig, provider: "off" });
  render();
  await settle();
  expect(button("Rotate Sync Password")).toBeUndefined();
});
