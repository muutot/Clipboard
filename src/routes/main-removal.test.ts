import { flushSync, mount, unmount } from "svelte";
import { get } from "svelte/store";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import MainPage from "./+page.svelte";
import { generalSettings } from "$lib/services/settings";
import { t } from "$lib/i18n";
import type { GeneralSettings, PersistedClipboardItem } from "$lib/types/clipboard";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  invoke: vi.fn(),
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (path: string) => path,
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (name, callback) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));

let target: HTMLDivElement;
let app: ReturnType<typeof mount>;
let savedSettings: GeneralSettings;
beforeEach(() => {
  savedSettings = get(generalSettings);
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (app) await unmount(app);
  target.remove();
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  generalSettings.set(savedSettings);
  bridge.listeners.clear();
  bridge.invoke.mockReset();
  vi.unstubAllGlobals();
});
async function settle() {
  for (let i = 0; i < 16; i++) {
    await Promise.resolve();
    flushSync();
  }
}

it.each([
  ["hard", "removed"],
  ["permanent", "removed"],
  ["hard", "invalidated"],
  ["permanent", "invalidated"],
  ["hard", "none"],
  ["permanent", "none"],
] as const)("reconciles a failed %s delete after %s confirmation", async (mode, event) => {
  generalSettings.set({ ...savedSettings, useRecycleBin: mode === "permanent" });
  app = mount(MainPage, { target });
  flushSync();
  const record = {
    id: "removal-race",
    kind: "text",
    title: "Removal race",
    textContent: "body",
    contentHash: "removal-hash",
    sourceApp: "test",
    sizeBytes: 4,
    createdAtMs: 1,
    lastUsedAtMs: 1,
    isFavorite: false,
  } as PersistedClipboardItem;
  bridge.listeners.get("clipboard-item-added")?.({ payload: record });
  if (mode === "permanent") {
    bridge.listeners.get("clipboard-items-changed")?.({ payload: { deletedIds: [record.id] } });
    flushSync();
    const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
      (button) => button.textContent?.trim() === t("filter.deleted"),
    );
    tab!.click();
  }
  flushSync();
  const row = () => target.querySelector<HTMLElement>(`[data-id="${record.id}"]`);
  row()!.dispatchEvent(new KeyboardEvent("keydown", { key: " ", bubbles: true }));
  await settle();
  expect(target.querySelector(".detail-panel")).not.toBeNull();
  let fail!: (error: Error) => void;
  const pending = new Promise<boolean>((_, reject) => (fail = reject));
  const command = mode === "hard" ? "delete_clipboard_item" : "permanently_delete_clipboard_item";
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  bridge.invoke.mockImplementation((name: string) =>
    name === command ? pending : new Promise(() => {}),
  );
  row()!
    .querySelector<HTMLButtonElement>(`button[aria-label="${t("card.delete")}"]`)!
    .click();
  await settle();
  expect(bridge.invoke).toHaveBeenCalledWith(command, { id: record.id });
  expect(row()).toBeNull();
  expect(target.querySelector(".detail-panel")).toBeNull();
  if (event === "removed") {
    bridge.listeners.get("clipboard-items-changed")?.({ payload: { removedIds: [record.id] } });
  } else if (event === "invalidated") {
    bridge.listeners.get("clipboard-history-invalidated")?.({
      payload: { deletedIds: [record.id] },
    });
  }
  fail(new Error("older request failed"));
  await settle();
  if (mode === "permanent") {
    // Removing the last bin row moves the actual route back to the all filter.
    // Re-select the bin before checking whether a failed rollback resurrected it.
    const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
      (button) => button.textContent?.trim() === t("filter.deleted"),
    );
    if (tab) tab.click();
    else expect(event).not.toBe("none");
    flushSync();
  }
  if (event === "none") {
    expect(row()).not.toBeNull();
    expect(target.querySelector(".detail-panel")).not.toBeNull();
  } else {
    expect(row()).toBeNull();
    expect(target.querySelector(".detail-panel")).toBeNull();
  }
});
