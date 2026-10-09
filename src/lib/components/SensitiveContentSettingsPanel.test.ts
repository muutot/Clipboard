import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./SensitiveContentSettingsPanel.svelte";
const bridge = vi.hoisted(() => ({ save: vi.fn() }));
vi.mock("$lib/services/capture", () => ({
  getPrivacySettings: vi
    .fn()
    .mockResolvedValue({ paused: false, localOnly: true, sensitivePatterns: ["stored"] }),
  setPrivacySettings: bridge.save,
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => false, getRuntimeInfo: vi.fn() }));
let target: HTMLDivElement;
let app: ReturnType<typeof mount>;
async function settle() {
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function edit(value: string) {
  const area = target.querySelector("textarea")!;
  area.value = value;
  area.dispatchEvent(new Event("input", { bubbles: true }));
}
const saveButton = () =>
  target.querySelector<HTMLButtonElement>(
    '[data-settings-search-id="capture.sensitive-patterns"] button',
  )!;
const localButton = () =>
  target.querySelector<HTMLButtonElement>('[data-settings-search-id="capture.local-only"] button')!;
beforeEach(async () => {
  bridge.save.mockReset().mockImplementation(async (patch) => ({
    paused: false,
    localOnly: true,
    sensitivePatterns: ["stored"],
    ...patch,
  }));
  target = document.createElement("div");
  document.body.append(target);
  app = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  await settle();
});
afterEach(async () => {
  await unmount(app);
  target.remove();
});
it("preserves unsaved sensitive patterns when local-only changes", async () => {
  edit("unsaved pattern");
  localButton().click();
  await settle();
  expect(bridge.save).toHaveBeenCalledWith({ localOnly: false });
  expect(localButton().getAttribute("aria-checked")).toBe("false");
  expect(target.querySelector("textarea")!.value).toBe("unsaved pattern");
});
it("retains edits made while saving and accepts normalization only for an unchanged draft", async () => {
  let finish!: (result: unknown) => void;
  bridge.save.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  edit("  submitted  ");
  saveButton().click();
  await settle();
  edit("newer draft");
  finish({ paused: false, localOnly: true, sensitivePatterns: ["submitted"] });
  await settle();
  expect(target.querySelector("textarea")!.value).toBe("newer draft");
  edit("  normalized  ");
  saveButton().click();
  await settle();
  expect(target.querySelector("textarea")!.value).toBe("normalized");
});
it("does not overwrite another setting with an older response snapshot", async () => {
  let finish!: (result: unknown) => void;
  bridge.save.mockReturnValueOnce(
    new Promise((resolve) => {
      finish = resolve;
    }),
  );
  edit("saved pattern");
  saveButton().click();
  await settle();
  localButton().click();
  await settle();
  finish({ paused: false, localOnly: true, sensitivePatterns: ["saved pattern"] });
  await settle();
  expect(localButton().getAttribute("aria-checked")).toBe("false");
});
