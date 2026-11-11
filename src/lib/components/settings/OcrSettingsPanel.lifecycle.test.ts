import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { locale, t } from "#lib/i18n/index.ts";
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
let feedback: (message: string, success: boolean) => void;

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
  feedback = vi.fn<(message: string, success: boolean) => void>();
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
  vi.useRealTimers();
  if (component) await unmount(component);
  target.remove();
});

it("serializes detection saves so the newest threshold wins", async () => {
  const requests = [deferred<unknown>(), deferred<unknown>()];
  let requestIndex = 0;
  vi.mocked(invoke).mockImplementation((command) => {
    if (command === "get_ocr_status") return Promise.resolve({ ...status });
    if (command === "get_ocr_config") return Promise.resolve({ ...config });
    if (command === "set_ocr_config") return requests[requestIndex++].promise;
    return Promise.resolve(undefined);
  });
  await render();
  const slider = target.querySelector<HTMLInputElement>('input[type="range"]')!;

  slider.value = "0.4";
  slider.dispatchEvent(new Event("input", { bubbles: true }));
  slider.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();
  slider.value = "0.5";
  slider.dispatchEvent(new Event("input", { bubbles: true }));
  slider.dispatchEvent(new Event("change", { bubbles: true }));
  await settle();

  const saves = () =>
    vi.mocked(invoke).mock.calls.filter(([command]) => command === "set_ocr_config");
  expect(saves()).toEqual([
    [
      "set_ocr_config",
      {
        settings: {
          engine: "ppocr",
          detScoreThreshold: 0.4,
          detBoxThreshold: 0.6,
          detUnclipRatio: 1.5,
        },
      },
    ],
  ]);

  requests[0].resolve({});
  await settle();
  expect(saves()).toEqual([
    [
      "set_ocr_config",
      {
        settings: {
          engine: "ppocr",
          detScoreThreshold: 0.4,
          detBoxThreshold: 0.6,
          detUnclipRatio: 1.5,
        },
      },
    ],
    [
      "set_ocr_config",
      {
        settings: {
          engine: "ppocr",
          detScoreThreshold: 0.5,
          detBoxThreshold: 0.6,
          detUnclipRatio: 1.5,
        },
      },
    ],
  ]);
  requests[1].resolve({});
  await settle();
});
it("refreshes full OCR status after applying a model during task polling", async () => {
  const poll = deferred<typeof status>();
  let statusReads = 0;
  let configReads = 0;
  let storedVariant = "tiny";
  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "get_ocr_status") {
      return statusReads++ === 0
        ? Promise.resolve({ ...status })
        : poll.promise.then((result) => ({
            ...result,
            ppocrModelVariant: storedVariant,
            installedVariants: [...result.installedVariants, storedVariant],
          }));
    }
    if (command === "get_ocr_config") {
      configReads += 1;
      return Promise.resolve({ ...config, ppocrModelVariant: storedVariant });
    }
    if (command === "set_ocr_config") {
      const settings = (args as { settings?: { ppocrModelVariant?: string } }).settings;
      storedVariant = settings?.ppocrModelVariant ?? storedVariant;
      return Promise.resolve(undefined);
    }
    return Promise.resolve(undefined);
  });
  vi.useFakeTimers();
  component = mount(Panel, { target, props: { onfeedback: feedback } });
  flushSync();
  await settle();
  expect(configReads).toBe(1);
  expect(target.querySelector('[data-settings-search-id="ocr.engine"]')).not.toBeNull();

  await vi.advanceTimersByTimeAsync(2000);
  await settle();
  expect(statusReads).toBe(2);

  try {
    choose(".ocr-model-select button", "small (~30MB) ✓");
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((entry) => entry.textContent?.trim() === t("storage.ocrModelApply"))!
      .click();
    await settle();
    expect(configReads).toBe(2);
  } finally {
    poll.resolve({ ...status });
    await settle();
  }
  const modelRow = target.querySelector('[data-settings-search-id="ocr.model"]')!;
  expect(modelRow.querySelector(".ocr-model-select button")?.textContent).toContain(
    "small (~30MB) ✓",
  );
  expect(
    [...modelRow.querySelectorAll<HTMLButtonElement>("button")].some(
      (button) => button.textContent?.trim() === t("storage.ocrModelApplied"),
    ),
  ).toBe(true);
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

it("keeps the latest full OCR refresh when saves overlap", async () => {
  const staleStatus = deferred<typeof status>();
  const staleConfig = deferred<typeof config>();
  let statusReads = 0;
  let configReads = 0;
  let storedEngine = "ppocr";
  let storedVariant = "tiny";
  let holdFirstRefresh = false;

  vi.mocked(invoke).mockImplementation((command, args) => {
    if (command === "get_ocr_status") {
      statusReads += 1;
      if (holdFirstRefresh && statusReads === 2) return staleStatus.promise;
      return Promise.resolve({
        ...status,
        engine: storedEngine,
        ppocrModelVariant: storedVariant,
        engineAvailable: storedEngine === "ppocr",
      });
    }
    if (command === "get_ocr_config") {
      configReads += 1;
      if (holdFirstRefresh && configReads === 2) return staleConfig.promise;
      return Promise.resolve({
        ...config,
        engine: storedEngine,
        ppocrModelVariant: storedVariant,
      });
    }
    if (command === "set_ocr_config") {
      const settings = (args as { settings?: { engine?: string; ppocrModelVariant?: string } })
        .settings;
      if (settings?.engine) storedEngine = settings.engine;
      if (settings?.ppocrModelVariant) storedVariant = settings.ppocrModelVariant;
      holdFirstRefresh = settings?.engine === "tesseract";
      return Promise.resolve(undefined);
    }
    return Promise.resolve(undefined);
  });

  await render();
  choose(`button[aria-label="${t("storage.ocrEngineLabel")}"]`, "Tesseract");
  await vi.waitFor(() =>
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "set_ocr_config"),
    ).toHaveLength(1),
  );
  await settle();

  choose(".ocr-model-select button", "small (~30MB) ✓");
  [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((entry) => entry.textContent?.trim() === t("storage.ocrModelApply"))!
    .click();
  await vi.waitFor(() =>
    expect(
      vi.mocked(invoke).mock.calls.filter(([command]) => command === "set_ocr_config"),
    ).toHaveLength(2),
  );

  staleStatus.resolve({
    ...status,
    engine: "tesseract",
    ppocrModelVariant: "tiny",
    engineAvailable: false,
  });
  staleConfig.resolve({
    ...config,
    engine: "tesseract",
    ppocrModelVariant: "tiny",
  });
  await settle();

  const modelRow = target.querySelector('[data-settings-search-id="ocr.model"]')!;
  expect(
    [...modelRow.querySelectorAll<HTMLButtonElement>("button")].some(
      (button) => button.textContent?.trim() === t("storage.ocrModelApplied"),
    ),
  ).toBe(true);
});
