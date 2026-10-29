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

function selectFilter(key: string) {
  const tab = [...target.querySelectorAll<HTMLButtonElement>('.filters [role="tab"]')].find(
    (button) => button.textContent?.trim() === t(key),
  );
  tab!.click();
  flushSync();
}

function seedMembershipRow() {
  const record = {
    id: "membership-race",
    kind: "text",
    title: "Membership race",
    textContent: "body",
    contentHash: "membership-hash",
    sourceApp: "test",
    sizeBytes: 4,
    createdAtMs: 1,
    lastUsedAtMs: 1,
    isFavorite: false,
  } as PersistedClipboardItem;
  bridge.listeners.get("clipboard-item-added")?.({ payload: record });
  flushSync();
  return record;
}

it.each(["delete", "restore"] as const)(
  "keeps a newer same-valued %s confirmation when an old membership request fails",
  async (action) => {
    generalSettings.set({ ...savedSettings, useRecycleBin: true });
    app = mount(MainPage, { target });
    flushSync();
    const record = seedMembershipRow();
    const confirm = action === "delete" ? "deletedIds" : "restoredIds";
    if (action === "restore") {
      bridge.listeners.get("clipboard-items-changed")?.({ payload: { deletedIds: [record.id] } });
      flushSync();
      selectFilter("filter.deleted");
    }
    let fail!: (error: Error) => void;
    const pending = new Promise<boolean>((_, reject) => (fail = reject));
    const command = action === "delete" ? "soft_delete_clipboard_item" : "restore_clipboard_item";
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    bridge.invoke.mockImplementation((name: string) =>
      name === command ? pending : new Promise(() => {}),
    );
    target
      .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
      .querySelector<HTMLButtonElement>(`button[aria-label="${t(`card.${action}`)}"]`)!
      .click();
    await settle();
    expect(bridge.invoke).toHaveBeenCalledWith(command, { id: record.id });
    bridge.listeners.get("clipboard-items-changed")?.({ payload: { [confirm]: [record.id] } });
    fail(new Error("older request failed"));
    await settle();
    if (action === "delete") {
      selectFilter("filter.all");
      expect(target.querySelector(`[data-id="${record.id}"]`)).toBeNull();
    }
    selectFilter(action === "delete" ? "filter.deleted" : "filter.all");
    expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
  },
);

it.each(["delete-first", "restore-first"] as const)(
  "restores confirmed active membership when delete and restore both fail (%s)",
  async (order) => {
    generalSettings.set({ ...savedSettings, useRecycleBin: true });
    app = mount(MainPage, { target });
    flushSync();
    const record = seedMembershipRow();
    let failDelete!: (error: Error) => void;
    let failRestore!: (error: Error) => void;
    const deletion = new Promise<boolean>((_, reject) => (failDelete = reject));
    const restoration = new Promise<boolean>((_, reject) => (failRestore = reject));
    void restoration.catch(() => {});
    Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
    bridge.invoke.mockImplementation((name: string) =>
      name === "soft_delete_clipboard_item"
        ? deletion
        : name === "restore_clipboard_item"
          ? restoration
          : new Promise(() => {}),
    );
    const click = (action: string) =>
      target
        .querySelector<HTMLElement>(`[data-id="${record.id}"]`)!
        .querySelector<HTMLButtonElement>(`button[aria-label="${t(`card.${action}`)}"]`)!
        .click();
    click("delete");
    await settle();
    selectFilter("filter.deleted");
    click("restore");
    await settle();
    if (order === "delete-first") {
      failDelete(new Error("delete failed"));
      await settle();
      failRestore(new Error("restore failed"));
    } else {
      failRestore(new Error("restore failed"));
      await settle();
      failDelete(new Error("delete failed"));
    }
    await settle();
    selectFilter("filter.all");
    expect(target.querySelector(`[data-id="${record.id}"]`)).not.toBeNull();
  },
);

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
