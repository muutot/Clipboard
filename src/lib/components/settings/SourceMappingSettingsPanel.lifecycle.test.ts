import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import Panel from "./SourceMappingSettingsPanel.svelte";

const bridge = vi.hoisted(() => ({
  get: vi.fn(),
  set: vi.fn(),
  refresh: vi.fn(),
  read: vi.fn(),
  feedback: vi.fn(),
}));

vi.mock("$lib/services/runtime", () => ({
  invokeTauri: vi.fn(async () => null),
  invokeTauriRequired: vi.fn(async () => null),
  isTauriRuntime: () => true,
  getRuntimeInfo: vi.fn(async () => null),
  openDevtools: vi.fn(),
  isDevtoolsHotkey: () => false,
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/clipboard", () => ({
  getSourceMappingRules: bridge.get,
  setSourceMappingRules: bridge.set,
}));
vi.mock("$lib/services/source-mapping.svelte", () => ({
  refreshSourceMappingRules: bridge.refresh,
  readSourceMappingRules: bridge.read,
}));

const rule = { domain: "github.com", titleKeyword: "Pull Request", name: "GitHub PRs" };
let target: HTMLDivElement;
let component: ReturnType<typeof mount> | null;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

beforeEach(async () => {
  vi.clearAllMocks();
  bridge.get.mockResolvedValue([{ ...rule }]);
  bridge.set.mockResolvedValue([{ ...rule }]);
  bridge.refresh.mockResolvedValue(undefined);
  bridge.read.mockReturnValue([{ ...rule }]);
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onclose: vi.fn(), onfeedback: bridge.feedback },
  });
  flushSync();
  await vi.waitFor(async () => {
    await settle();
    expect(target.querySelector<HTMLInputElement>(".mapping-domain")).not.toBeNull();
  });
});

afterEach(async () => {
  if (component) await unmount(component);
  target.remove();
});

it.each(["succeeded", "failed"] as const)(
  "does not publish a save that %s after teardown",
  async (outcome) => {
    let finish!: (value: (typeof rule)[]) => void;
    let fail!: (error: Error) => void;
    bridge.set.mockReturnValueOnce(
      new Promise((resolve, reject) => {
        finish = resolve;
        fail = reject;
      }),
    );
    target.querySelector<HTMLButtonElement>(".mapping-save")!.click();
    await vi.waitFor(() => expect(bridge.set).toHaveBeenCalled());

    await unmount(component!);
    component = null;
    if (outcome === "failed") fail(new Error("late save failure"));
    else finish([{ ...rule }]);
    await settle();
    expect(bridge.feedback).not.toHaveBeenCalled();
    if (outcome === "succeeded") expect(bridge.refresh).not.toHaveBeenCalled();
  },
);
