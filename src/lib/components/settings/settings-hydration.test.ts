import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import OcrSettingsPanel from "./OcrSettingsPanel.svelte";
import IgnoredAppsSettingsPanel from "./IgnoredAppsSettingsPanel.svelte";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), storage: vi.fn(), feedback: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (v: string) => v,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn().mockResolvedValue(() => {}) }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false, maxTextCaptureBytes: 512000 }) };
});
vi.mock("$lib/services/storage", () => ({ getStorageConfig: bridge.storage }));
vi.mock("$lib/services/capture", () => ({
  getApplicationFilterSettings: vi
    .fn()
    .mockResolvedValue({ ignoredApplications: [], discoveredApplications: [] }),
  configureIgnoredApplications: vi.fn(),
}));
const status = {
  totalTasks: 4,
  pendingTasks: 1,
  completedTasks: 3,
  failedTasks: 0,
  engine: "ppocr",
  engineAvailable: true,
  hasEngine: true,
  ppocrModelVariant: "medium",
  installedVariants: ["medium"],
};
const config = {
  engine: "ppocr",
  ppocrModelVariant: "medium",
  detScoreThreshold: 0.45,
  detBoxThreshold: 0.8,
  detUnclipRatio: 2.1,
};
const cleanups: (() => Promise<void>)[] = [];
beforeEach(() => {
  bridge.invoke
    .mockReset()
    .mockImplementation(async (command) => (command === "get_ocr_status" ? status : config));
  bridge.storage.mockReset().mockResolvedValue({ maxFileCopySizeBytes: 83 * 1024 * 1024 });
  bridge.feedback.mockReset();
});
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});
async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function render(kind: "ocr" | "files") {
  const target = document.createElement("div");
  document.body.append(target);
  const app =
    kind === "ocr"
      ? mount(OcrSettingsPanel, { target, props: { onfeedback: bridge.feedback } })
      : mount(IgnoredAppsSettingsPanel, {
          target,
          props: { onclose: () => {}, showHeader: false },
        });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}
function retry(target: HTMLElement) {
  target.querySelector<HTMLButtonElement>(".settings-state button")!.click();
}
it("keeps OCR defaults inaccessible after failed config hydration and loads saved thresholds on retry", async () => {
  let fail = true;
  bridge.invoke.mockImplementation(async (command) => {
    if (command === "get_ocr_config" && fail) throw new Error("fixture config failure");
    return command === "get_ocr_status" ? status : config;
  });
  const target = render("ocr");
  await settle();
  expect(target.querySelector('input[type="range"]')).toBeNull();
  expect(target.querySelector('[data-settings-search-id="ocr.engine"]')).toBeNull();
  fail = false;
  retry(target);
  await settle();
  const sliders = Array.from(target.querySelectorAll<HTMLInputElement>('input[type="range"]'));
  expect(sliders.map((s) => Number(s.value))).toEqual([0.45, 0.8, 2.1]);
  sliders[0].value = "0.5";
  sliders[0].dispatchEvent(new Event("input", { bubbles: true }));
  sliders[0].dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  expect(bridge.invoke).toHaveBeenCalledWith("set_ocr_config", {
    settings: {
      engine: "ppocr",
      detScoreThreshold: 0.5,
      detBoxThreshold: 0.8,
      detUnclipRatio: 2.1,
    },
  });
});
it.each(["rejected", "null"])(
  "keeps the copy limit gated when storage is %s and retries without hiding text settings",
  async (failure) => {
    if (failure === "rejected")
      bridge.storage.mockRejectedValueOnce(new Error("fixture storage failure"));
    else bridge.storage.mockResolvedValueOnce(null);
    const target = render("files");
    await settle();
    expect(
      target.querySelector('[data-settings-search-id="storage.max-file-copy-size"]'),
    ).toBeNull();
    expect(
      target.querySelector('[data-settings-search-id="general.max-text-capture-size"] input'),
    ).not.toBeNull();
    retry(target);
    await settle();
    expect(
      target.querySelector<HTMLInputElement>(
        '[data-settings-search-id="storage.max-file-copy-size"] input',
      )!.value,
    ).toBe("83");
    expect(bridge.invoke).not.toHaveBeenCalledWith("set_storage_config", expect.anything());
  },
);
it("does not publish late OCR load failures after teardown", async () => {
  let reject!: (error: Error) => void;
  bridge.invoke.mockImplementation((command) =>
    command === "get_ocr_config"
      ? new Promise((_, fail) => {
          reject = fail;
        })
      : Promise.resolve(status),
  );
  const target = render("ocr");
  expect(target.querySelector('input[type="range"]')).toBeNull();
  await cleanups.pop()!();
  reject(new Error("late fixture failure"));
  await settle();
  expect(bridge.feedback).not.toHaveBeenCalled();
});
it("keeps task polling from replacing an edited detection threshold", async () => {
  vi.useFakeTimers();
  try {
    const target = render("ocr");
    await settle();
    const slider = target.querySelector<HTMLInputElement>('input[type="range"]')!;
    slider.value = "0.55";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    await vi.advanceTimersByTimeAsync(2000);
    await settle();
    expect(slider.value).toBe("0.55");
    expect(
      bridge.invoke.mock.calls.filter(([command]) => command === "get_ocr_config"),
    ).toHaveLength(1);
    expect(
      bridge.invoke.mock.calls.filter(([command]) => command === "get_ocr_status"),
    ).toHaveLength(2);
  } finally {
    vi.useRealTimers();
  }
});
