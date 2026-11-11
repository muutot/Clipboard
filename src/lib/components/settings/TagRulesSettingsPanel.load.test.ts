import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { locale, t } from "#lib/i18n/index.ts";
import Panel from "./TagRulesSettingsPanel.svelte";
import * as service from "#lib/services/clipboard.ts";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(async () => null) }));

vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("#lib/services/runtime.ts", () => ({
  invokeTauri: bridge.invoke,
  invokeTauriRequired: bridge.invoke,
  isTauriRuntime: () => true,
  getRuntimeInfo: vi.fn(async () => null),
  openDevtools: vi.fn(),
  isDevtoolsHotkey: () => false,
}));
vi.mock("#lib/services/settings.ts", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});
vi.mock("#lib/services/clipboard.ts", () => ({
  getAutoTagRules: vi.fn(),
  setAutoTagRules: vi.fn(),
  previewAutoTagRules: vi.fn(),
  previewAutoTagHistory: vi.fn(),
  applyAutoTagHistory: vi.fn(),
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | null;

async function settle() {
  for (let index = 0; index < 20; index++) {
    await Promise.resolve();
    flushSync();
  }
}

afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  target.remove();
  vi.clearAllMocks();
});

it("blocks saving empty rules when the existing configuration fails to load", async () => {
  locale.set("en");
  vi.mocked(service.getAutoTagRules).mockRejectedValue(new Error("database unavailable"));
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, { target, props: { onclose: vi.fn() } });
  flushSync();
  await settle();

  expect(target.textContent).toContain(t("tags.loadFailed"));
  const saveButton = target.querySelector<HTMLButtonElement>(".autotag-save");
  expect(saveButton === null || saveButton.disabled).toBe(true);
  expect(service.setAutoTagRules).not.toHaveBeenCalled();
});
