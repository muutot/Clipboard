import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { locale, t } from "$lib/i18n";
import Panel from "./OcrSettingsPanel.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const status = {
  totalTasks: 0,
  pendingTasks: 0,
  completedTasks: 0,
  failedTasks: 0,
  engine: "ppocr",
  engineAvailable: true,
  hasEngine: true,
  ppocrModelVariant: "tiny",
  installedVariants: ["tiny", "small"],
};

const config = {
  engine: "ppocr",
  ppocrModelVariant: "tiny",
  detScoreThreshold: 0.3,
  detBoxThreshold: 0.6,
  detUnclipRatio: 1.5,
};

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | null;
let feedback: ReturnType<typeof vi.fn>;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function render() {
  component = mount(Panel, { target, props: { onfeedback: feedback } });
  flushSync();
  await vi.waitFor(async () => {
    await settle();
    expect(target.querySelector('[data-settings-search-id="ocr.engine"]')).not.toBeNull();
  });
}

async function teardown() {
  await unmount(component!);
  component = null;
}

function choose(triggerSelector: string, option: string) {
  target.querySelector<HTMLButtonElement>(triggerSelector)!.click();
  flushSync();
  [...target.querySelectorAll<HTMLButtonElement>('[role="option"]')]
    .find((entry) => entry.textContent?.trim() === option)!
    .click();
  flushSync();
}

beforeEach(() => {
  vi.clearAllMocks();
  feedback = vi.fn();
  vi.mocked(listen).mockResolvedValue(vi.fn());
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "get_ocr_status") return { ...status };
    if (command === "get_ocr_config") return { ...config };
    return undefined;
  });
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
});

afterEach(async () => {
  if (component) await unmount(component);
  target.remove();
});

it.each(["succeeded", "failed"] as const)(
  "does not publish an OCR detection save that %s after teardown",
  async (outcome) => {
    const request = deferred<unknown>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_ocr_status") return { ...status };
      if (command === "get_ocr_config") return { ...config };
      if (command === "set_ocr_config") return request.promise;
      return undefined;
    });
    await render();
    const slider = target.querySelector<HTMLInputElement>('input[type="range"]')!;
    slider.value = "0.4";
    slider.dispatchEvent(new Event("input", { bubbles: true }));
    slider.dispatchEvent(new Event("change", { bubbles: true }));
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_ocr_config", expect.anything()),
    );
    await teardown();
    if (outcome === "failed") request.reject(new Error("late OCR failure"));
    else request.resolve(undefined);
    await settle();
    expect(feedback).not.toHaveBeenCalled();
  },
);

it.each(["succeeded", "failed"] as const)(
  "does not publish an OCR model apply that %s after teardown",
  async (outcome) => {
    const request = deferred<unknown>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_ocr_status") return { ...status };
      if (command === "get_ocr_config") return { ...config };
      if (command === "set_ocr_config") return request.promise;
      return undefined;
    });
    await render();
    choose(".ocr-model-select button", "small (~30MB) ✓");
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === t("storage.ocrModelApply"))!
      .click();
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_ocr_config", expect.anything()),
    );
    await teardown();
    if (outcome === "failed") request.reject(new Error("late model failure"));
    else request.resolve(undefined);
    await settle();
    expect(feedback).not.toHaveBeenCalled();
  },
);

it.each(["succeeded", "failed"] as const)(
  "does not publish an OCR engine change that %s after teardown",
  async (outcome) => {
    const request = deferred<unknown>();
    vi.mocked(invoke).mockImplementation(async (command) => {
      if (command === "get_ocr_status") return { ...status };
      if (command === "get_ocr_config") return { ...config };
      if (command === "set_ocr_config") return request.promise;
      return undefined;
    });
    await render();
    choose(`button[aria-label="${t("storage.ocrEngineLabel")}"]`, "Tesseract");
    await vi.waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("set_ocr_config", expect.anything()),
    );
    await teardown();
    if (outcome === "failed") request.reject(new Error("late engine failure"));
    else request.resolve(undefined);
    await settle();
    expect(feedback).not.toHaveBeenCalled();
  },
);
