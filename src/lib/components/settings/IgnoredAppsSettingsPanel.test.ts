import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./IgnoredAppsSettingsPanel.svelte";

const bridge = vi.hoisted(() => ({ configure: vi.fn() }));
vi.mock("$lib/services/capture", () => ({
  getApplicationFilterSettings: vi.fn().mockResolvedValue({
    discoveredApplications: [],
    discoveredApplicationsWithIcons: [],
    ignoredApplications: [],
  }),
  configureIgnoredApplications: bridge.configure,
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false, maxTextCaptureBytes: 512000 }) };
});
vi.mock("$lib/services/storage", () => ({
  getStorageConfig: vi.fn().mockResolvedValue({ maxFileCopySizeBytes: 50 * 1024 * 1024 }),
}));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => false }));

let target: HTMLDivElement;
let app: ReturnType<typeof mount>;

async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}

function setManualApplication(value: string) {
  const input = target.querySelector<HTMLInputElement>(".manual-add input")!;
  input.value = value;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
  return input;
}

function submitManualApplication() {
  target
    .querySelector<HTMLInputElement>(".manual-add input")!
    .dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
}

beforeEach(async () => {
  bridge.configure.mockReset();
  target = document.createElement("div");
  document.body.append(target);
  app = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  await settle();
});

afterEach(async () => {
  await unmount(app);
  target.remove();
});

it("does not start overlapping manual application saves from Enter", async () => {
  const finishes: ((result: string[]) => void)[] = [];
  bridge.configure.mockImplementation(
    () =>
      new Promise((resolve) => {
        finishes.push(resolve);
      }),
  );

  setManualApplication("alpha.exe");
  submitManualApplication();
  await settle();
  expect(bridge.configure).toHaveBeenCalledWith(["alpha.exe"]);

  setManualApplication("bravo.exe");
  submitManualApplication();
  await settle();
  expect(bridge.configure).toHaveBeenCalledTimes(1);

  finishes[0](["alpha.exe"]);
  await settle();
  expect(bridge.configure).toHaveBeenCalledTimes(1);
});
