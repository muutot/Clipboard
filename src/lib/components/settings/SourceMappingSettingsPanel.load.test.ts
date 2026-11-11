import { flushSync, mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { locale, t } from "#lib/i18n/index.ts";
import Panel from "./SourceMappingSettingsPanel.svelte";
import * as clipboard from "#lib/services/clipboard.ts";
import { destroySourceMappingRules } from "#lib/services/source-mapping.svelte.ts";

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
  getSourceMappingRules: vi.fn(),
  setSourceMappingRules: vi.fn(),
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
  destroySourceMappingRules();
  target.remove();
  vi.clearAllMocks();
});

it("blocks saving empty mappings when the existing rules fail to load", async () => {
  locale.set("en");
  vi.mocked(clipboard.getSourceMappingRules).mockRejectedValue(new Error("database unavailable"));
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onclose: vi.fn(), onfeedback: vi.fn() },
  });
  flushSync();
  await settle();

  expect(target.textContent).toContain(t("storage.configLoadFailed"));
  const saveButton = target.querySelector<HTMLButtonElement>(".mapping-save");
  expect(saveButton === null || saveButton.disabled).toBe(true);
  expect(clipboard.setSourceMappingRules).not.toHaveBeenCalled();
});
