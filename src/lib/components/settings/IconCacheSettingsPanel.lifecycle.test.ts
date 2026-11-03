import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import Panel from "./IconCacheSettingsPanel.svelte";

const bridge = vi.hoisted(() => ({
  open: vi.fn(),
  list: vi.fn(),
  delete: vi.fn(),
  replace: vi.fn(),
  feedback: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ convertFileSrc: (path: string) => `asset://${path}` }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: bridge.open }));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/storage", () => ({
  listIconCache: bridge.list,
  deleteIconFiles: bridge.delete,
  replaceIconFile: bridge.replace,
}));

const iconEntries = [
  {
    appName: "alpha",
    displayName: "Alpha",
    firstChar: "A",
    iconName: "alpha.png",
    contentHash: "hash-alpha",
    targetIconName: "alpha.png",
    sizeBytes: 32,
  },
  {
    appName: "beta",
    displayName: "Beta",
    firstChar: "B",
    iconName: "beta.png",
    contentHash: "hash-beta",
    targetIconName: "beta.png",
    sizeBytes: 64,
  },
];

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | null;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

async function waitForList() {
  await vi.waitFor(async () => {
    await settle();
    expect(target.querySelectorAll(".icon-file-item")).toHaveLength(2);
  });
}

async function startDelete() {
  await waitForList();
  target.querySelector<HTMLInputElement>(".icon-file-item input")!.click();
  flushSync();
  target.querySelector<HTMLButtonElement>(".icon-actions > button")!.click();
  await vi.waitFor(() => expect(bridge.delete).toHaveBeenCalled());
}

async function startReplacement() {
  await waitForList();
  target.querySelector<HTMLButtonElement>(".icon-replace-btn")!.click();
  flushSync();
  target.querySelector<HTMLButtonElement>(".icon-replace-file-btn")!.click();
  await vi.waitFor(() => expect(bridge.open).toHaveBeenCalled());
}

beforeEach(() => {
  vi.clearAllMocks();
  bridge.open.mockResolvedValue("replacement.png");
  bridge.list.mockResolvedValue(iconEntries);
  bridge.delete.mockResolvedValue(1);
  bridge.replace.mockResolvedValue(undefined);
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { iconsDir: "B:/icons", onfeedback: bridge.feedback },
  });
  flushSync();
});

afterEach(async () => {
  if (component) await unmount(component);
  target.remove();
});

it.each(["succeeded", "failed"] as const)(
  "does not publish a delete that %s after teardown",
  async (outcome) => {
    let finish!: (value: number) => void;
    let fail!: (error: Error) => void;
    bridge.delete.mockReturnValueOnce(
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    await startDelete();
    await unmount(component!);
    component = null;
    if (outcome === "failed") fail(new Error("late delete failure"));
    else finish(1);
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
    expect(bridge.list).toHaveBeenCalledTimes(1);
  },
);

it.each(["succeeded", "failed"] as const)(
  "does not publish a replacement that %s after teardown",
  async (outcome) => {
    let finish!: () => void;
    let fail!: (error: Error) => void;
    bridge.replace.mockReturnValueOnce(
      new Promise<void>((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    await startReplacement();
    await vi.waitFor(() => expect(bridge.replace).toHaveBeenCalled());
    bridge.feedback.mockClear();
    await unmount(component!);
    component = null;
    if (outcome === "failed") fail(new Error("late replace failure"));
    else finish();
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
    expect(bridge.list).toHaveBeenCalledTimes(1);
  },
);

it("does not start replacement when the file dialog resolves after teardown", async () => {
  let finish!: (value: string) => void;
  bridge.open.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  await startReplacement();
  await unmount(component!);
  component = null;
  finish("replacement.png");
  await settle();
  expect(bridge.replace).not.toHaveBeenCalled();
  expect(bridge.feedback).not.toHaveBeenCalled();
});
