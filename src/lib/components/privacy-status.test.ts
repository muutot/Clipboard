import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./SensitiveContentSettingsPanel.svelte";
const bridge = vi.hoisted(() => ({ invoke: vi.fn(), listen: vi.fn(), unlisten: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: bridge.listen }));
vi.mock("$lib/services/capture", () => ({
  getPrivacySettings: vi
    .fn()
    .mockResolvedValue({ paused: false, localOnly: true, sensitivePatterns: [] }),
  setPrivacySettings: vi.fn(),
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/runtime", () => ({
  isTauriRuntime: () => true,
  getRuntimeInfo: vi.fn().mockResolvedValue({ operatingSystem: "windows" }),
}));
let target: HTMLDivElement;
let app: ReturnType<typeof mount> | undefined;
let emit: (event: { payload: boolean }) => void;
async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}
const pauseButton = () =>
  target.querySelector<HTMLButtonElement>('[data-settings-search-id="recording.pause"] button');
beforeEach(() => {
  bridge.invoke.mockReset().mockResolvedValue({ paused: false });
  bridge.unlisten.mockReset();
  bridge.listen.mockReset().mockImplementation(async (_, callback) => {
    emit = callback;
    return bridge.unlisten;
  });
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  target.remove();
});
function render() {
  app = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  flushSync();
}
it("registers the listener before requesting the snapshot and preserves newer pause events", async () => {
  let registered!: (value: () => void) => void;
  bridge.listen.mockImplementation((_, callback) => {
    emit = callback;
    return new Promise((resolve) => {
      registered = resolve;
    });
  });
  let snapshot!: (value: { paused: boolean }) => void;
  bridge.invoke.mockReturnValue(
    new Promise((resolve) => {
      snapshot = resolve;
    }),
  );
  render();
  await settle();
  expect(bridge.invoke).not.toHaveBeenCalled();
  registered(bridge.unlisten);
  await settle();
  expect(bridge.invoke).toHaveBeenCalledWith("get_privacy_status");
  emit({ payload: true });
  snapshot({ paused: false });
  await settle();
  expect(pauseButton()!.getAttribute("aria-checked")).toBe("false");
});
it("does not present active recording after a failed status read and can retry", async () => {
  bridge.invoke.mockRejectedValueOnce(new Error("fixture status failure"));
  render();
  await settle();
  expect(pauseButton()).toBeNull();
  bridge.invoke.mockResolvedValue({ paused: true });
  target.querySelector<HTMLButtonElement>(".settings-state button")!.click();
  await settle();
  expect(pauseButton()!.getAttribute("aria-checked")).toBe("false");
});
it("cleans a listener that registers after unmount without starting a snapshot", async () => {
  let registered!: (value: () => void) => void;
  bridge.listen.mockReturnValue(
    new Promise((resolve) => {
      registered = resolve;
    }),
  );
  render();
  await settle();
  await unmount(app!);
  app = undefined;
  registered(bridge.unlisten);
  await settle();
  expect(bridge.unlisten).toHaveBeenCalledOnce();
  expect(bridge.invoke).not.toHaveBeenCalled();
});
