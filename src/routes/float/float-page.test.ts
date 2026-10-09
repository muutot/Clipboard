import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ClipboardItem, ClipboardItemsChangedPayload } from "$lib/types/clipboard";
import { generalSettings } from "$lib/services/settings";
import FloatPage from "./+page.svelte";

const bridge = vi.hoisted(() => ({
  listeners: new Map<string, (event: { payload: unknown }) => void>(),
  load: vi.fn(),
  favorite: vi.fn(),
  softDelete: vi.fn(),
  hardDelete: vi.fn(),
  copy: vi.fn(),
  emit: vi.fn(),
  showMain: vi.fn(),
  focusMain: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => ({
  emitTo: bridge.emit,
  listen: vi.fn(async (name, callback) => {
    bridge.listeners.set(name, callback);
    return () => bridge.listeners.delete(name);
  }),
}));
vi.mock("@tauri-apps/api/webviewWindow", () => ({
  WebviewWindow: {
    getByLabel: vi.fn(async (label) =>
      label === "main" ? { show: bridge.showMain, setFocus: bridge.focusMain } : null,
    ),
  },
}));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => null }));
vi.mock("$lib/services/runtime", async (original) => ({
  ...(await original<object>()),
  isTauriRuntime: () => true,
}));
vi.mock("$lib/services/settings-bootstrap", () => ({ applyGeneralSettingsToDocument: () => {} }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return {
    generalSettings: writable({
      colorIcons: false,
      showToastNotifications: false,
      pinCopiedToTop: true,
      floatPanelLeftClick: "favorite",
      floatPanelRightClick: "none",
      floatPanelMiddleClick: "none",
    }),
  };
});
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<object>()),
  loadClipboardHistory: bridge.load,
  persistFavorite: bridge.favorite,
  persistDelete: bridge.softDelete,
  persistHardDelete: bridge.hardDelete,
  copyClipboardItem: bridge.copy,
}));

function item(id: string, favorite = false): ClipboardItem {
  return {
    id,
    kind: "text",
    title: id,
    preview: id,
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "1 B",
    createdAt: 0,
    favorite,
  };
}

const cleanups: (() => Promise<void>)[] = [];
beforeEach(() => {
  generalSettings.update((settings) => ({ ...settings, floatPanelLeftClick: "favorite" }));
  bridge.listeners.clear();
  bridge.load.mockReset().mockResolvedValue([item("a", true)]);
  bridge.favorite.mockReset().mockResolvedValue(true);
  bridge.softDelete.mockReset().mockResolvedValue(true);
  bridge.hardDelete.mockReset().mockResolvedValue(true);
  bridge.copy.mockReset();
  bridge.emit.mockReset().mockResolvedValue(undefined);
  bridge.showMain.mockReset().mockResolvedValue(undefined);
  bridge.focusMain.mockReset().mockResolvedValue(undefined);
});
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
});

async function settle() {
  for (let i = 0; i < 5; i++) {
    await Promise.resolve();
    flushSync();
  }
}

async function render() {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(FloatPage, { target });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  await settle();
  return {
    target,
    rows: () => [...target.querySelectorAll<HTMLButtonElement>(".float-row")],
    favorites: async () => {
      target.querySelectorAll<HTMLButtonElement>('[role="tab"]')[1].click();
      await settle();
    },
  };
}

function changed(payload: Partial<ClipboardItemsChangedPayload>) {
  bridge.listeners.get("clipboard-items-changed")?.({ payload });
}

describe("float history reconciliation", () => {
  it.each([true, false])("honors useRecycleBin=%s when deleting", async (useRecycleBin) => {
    generalSettings.update((settings) => ({
      ...settings,
      useRecycleBin,
      floatPanelLeftClick: "delete",
    }));
    bridge.load.mockResolvedValue([item("a")]);
    const page = await render();
    page.rows()[0].click();
    await settle();
    expect(useRecycleBin ? bridge.softDelete : bridge.hardDelete).toHaveBeenCalledWith("a");
    expect(useRecycleBin ? bridge.hardDelete : bridge.softDelete).not.toHaveBeenCalled();
    expect(page.rows()).toHaveLength(0);
  });

  it("retains the row when permanent deletion fails", async () => {
    generalSettings.update((settings) => ({
      ...settings,
      useRecycleBin: false,
      floatPanelLeftClick: "delete",
    }));
    bridge.load.mockResolvedValue([item("a")]);
    bridge.hardDelete.mockResolvedValue(false);
    const page = await render();
    page.rows()[0].click();
    await settle();
    expect(page.rows()).toHaveLength(1);
  });

  it("opens details in the main window without copying the record", async () => {
    generalSettings.update((settings) => ({ ...settings, floatPanelLeftClick: "detail" }));
    const page = await render();
    page.rows()[0].click();
    await settle();
    expect(bridge.copy).not.toHaveBeenCalled();
    expect(bridge.emit).toHaveBeenCalledWith(
      "main",
      "clipboard-open-detail",
      expect.objectContaining({ id: "a" }),
    );
    expect(bridge.showMain).toHaveBeenCalledOnce();
    expect(bridge.focusMain).toHaveBeenCalledOnce();
  });

  it("keeps each copying row disabled until its own operation completes", async () => {
    generalSettings.update((settings) => ({ ...settings, floatPanelLeftClick: "copy" }));
    bridge.load.mockResolvedValue([item("a"), item("b")]);
    let finishA!: () => void;
    let finishB!: () => void;
    bridge.copy
      .mockReturnValueOnce(
        new Promise<void>((resolve) => {
          finishA = resolve;
        }),
      )
      .mockReturnValueOnce(
        new Promise<void>((resolve) => {
          finishB = resolve;
        }),
      );
    const page = await render();
    page.rows()[0].click();
    await settle();
    page.rows()[1].click();
    await settle();
    expect(page.rows().map((row) => row.disabled)).toEqual([true, true]);
    finishB();
    await settle();
    expect(page.rows().map((row) => row.disabled)).toEqual([true, false]);
    page.rows()[0].click();
    expect(bridge.copy).toHaveBeenCalledTimes(2);
    finishA();
    await settle();
    expect(page.rows().map((row) => row.disabled)).toEqual([false, false]);
  });

  it("hides a soft-deleted row immediately while its replacement page is pending", async () => {
    const page = await render();
    expect(page.rows()).toHaveLength(1);
    bridge.load.mockReturnValue(new Promise(() => {}));
    changed({ deletedIds: ["a"] });
    await settle();
    expect(page.rows()).toHaveLength(0);
  });

  it("removes a row when it is unfavorited in the favorites tab", async () => {
    const page = await render();
    await page.favorites();
    page.rows()[0].click();
    await settle();
    expect(bridge.favorite).toHaveBeenCalledWith("a", false);
    expect(page.rows()).toHaveLength(0);
  });

  it("loads a restored row that was never present in this window", async () => {
    bridge.load.mockResolvedValue([]);
    const page = await render();
    bridge.load.mockResolvedValue([item("restored")]);
    changed({ restoredIds: ["restored"] });
    await settle();
    expect(page.rows().map((row) => row.title)).toEqual(["restored"]);
  });

  it("refreshes after imports and sync invalidate history", async () => {
    const page = await render();
    bridge.load.mockResolvedValue([item("imported")]);
    bridge.listeners.get("clipboard-history-invalidated")?.({ payload: { deletedIds: [] } });
    await settle();
    expect(page.rows().map((row) => row.title)).toEqual(["imported"]);
  });

  it("discards a pending page when a newer deletion reload has completed", async () => {
    const page = await render();
    let completeOldPage!: (items: ClipboardItem[]) => void;
    bridge.load.mockReturnValueOnce(
      new Promise<ClipboardItem[]>((resolve) => {
        completeOldPage = resolve;
      }),
    );
    bridge.listeners.get("clipboard-item-added")?.({ payload: {} });
    bridge.load.mockResolvedValue([]);
    changed({ deletedIds: ["a"] });
    await settle();
    completeOldPage([item("a")]);
    await settle();
    expect(page.rows()).toHaveLength(0);
  });
});
