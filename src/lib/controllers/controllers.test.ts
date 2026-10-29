import { mount, unmount, flushSync } from "svelte";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import Probe, { type ProbeState } from "./controllers.probe.svelte";
import { createListenerScope } from "./window-lifecycle";
import * as service from "$lib/services/clipboard";
import { showToast } from "$lib/services/toast";
import {
  appendItems,
  applyItemPatches,
  removeItems,
  setDetailItem,
} from "$lib/utils/store/item-store";
import { applyItemsChangedEvent } from "$lib/utils/store/item-changes";
import type { ClipboardItem } from "$lib/types/clipboard";
vi.mock("$lib/services/runtime", async (original) => ({
  ...(await original<typeof import("$lib/services/runtime")>()),
  isTauriRuntime: () => true,
}));
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  loadClipboardHistory: vi.fn(),
  loadDeletedClipboardHistory: vi.fn(async () => []),
  searchClipboardHistory: vi.fn(),
  persistBatchFavorite: vi.fn(),
  persistBatchDelete: vi.fn(),
  persistBatchPermanentDelete: vi.fn(),
  pasteClipboardItem: vi.fn(async () => "pasted" as const),
}));
vi.mock("$lib/services/toast", () => ({ showToast: vi.fn() }));
const item = (id: string): ClipboardItem => ({
  id,
  kind: "text",
  title: id,
  preview: id,
  textContent: id,
  sourceApp: "test",
  sourceTone: "neutral",
  sizeLabel: "1 B",
  createdAt: 1,
  lastUsedAtMs: 1,
  favorite: false,
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
let out: ProbeState;
let target: HTMLDivElement;
let app: ReturnType<typeof mount> | undefined;
async function settle(ms = 0) {
  flushSync();
  await vi.advanceTimersByTimeAsync(ms);
  for (let i = 0; i < 8; i++) {
    await Promise.resolve();
    flushSync();
  }
}
beforeEach(() => {
  vi.useFakeTimers();
  out = {};
  target = document.createElement("div");
  document.body.append(target);
  app = mount(Probe, { target, props: { out, initial: [item("initial")] } });
  flushSync();
});
afterEach(async () => {
  if (app) await unmount(app);
  target.remove();
  vi.useRealTimers();
  vi.clearAllMocks();
});
it("rejects stale history pages after invalidation and after unmount", async () => {
  const old = deferred<ClipboardItem[]>(),
    next = deferred<ClipboardItem[]>();
  vi.mocked(service.loadClipboardHistory)
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(next.promise);
  void out.history!.loadActiveHistoryPage();
  out.history!.invalidateActiveHistoryPagination();
  next.resolve([item("current")]);
  await settle();
  old.resolve([item("stale")]);
  await settle();
  expect(out.store!.current.historyIds).toEqual(["current"]);
  const late = deferred<ClipboardItem[]>();
  vi.mocked(service.loadClipboardHistory).mockReturnValueOnce(late.promise);
  out.history!.invalidateActiveHistoryPagination();
  await unmount(app!);
  app = undefined;
  late.resolve([item("late")]);
  await settle();
  expect(out.store!.current.historyIds).toEqual(["current"]);
});
it("keeps only the latest debounced query and ignores responses after disposal", async () => {
  const old = deferred<Awaited<ReturnType<typeof service.searchClipboardHistory>>>(),
    next = deferred<Awaited<ReturnType<typeof service.searchClipboardHistory>>>();
  vi.mocked(service.searchClipboardHistory)
    .mockReturnValueOnce(old.promise)
    .mockReturnValueOnce(next.promise);
  out.setQuery!("old");
  await settle(301);
  out.setQuery!("new");
  await settle(301);
  next.resolve({ items: [item("new")], totalCount: 1, truncated: false });
  await settle();
  old.resolve({ items: [item("old")], totalCount: 1, truncated: false });
  await settle();
  expect(out.store!.current.indexedIds).toEqual(["new"]);
  const late = deferred<Awaited<ReturnType<typeof service.searchClipboardHistory>>>();
  vi.mocked(service.searchClipboardHistory).mockReturnValueOnce(late.promise);
  out.setQuery!("late");
  await settle(301);
  await unmount(app!);
  app = undefined;
  late.resolve({ items: [item("late")], totalCount: 1, truncated: false });
  await settle();
  expect(out.store!.current.indexedIds).toEqual(["new"]);
});
it("rolls back a failed bulk favorite without losing a concurrent capture", async () => {
  const result = deferred<boolean>();
  vi.mocked(service.persistBatchFavorite).mockReturnValueOnce(result.promise);
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkFavorite();
  expect(out.store!.current.byId.get("initial")!.favorite).toBe(true);
  out.store!.current = appendItems(out.store!.current, [item("arrived")], "history");
  result.resolve(false);
  await settle();
  expect(out.store!.current.byId.get("initial")!.favorite).toBe(false);
  expect(out.store!.current.byId.has("arrived")).toBe(true);
});
it("keeps a later successful edit when an earlier bulk favorite fails", async () => {
  const result = deferred<boolean>();
  vi.mocked(service.persistBatchFavorite).mockReturnValueOnce(result.promise);
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkFavorite();
  // The same item-store patch used by saveEdit after its persistence succeeds.
  out.store!.current = applyItemPatches(
    out.store!.current,
    new Map([
      [
        "initial",
        {
          title: "saved title",
          textContent: "saved body",
          preview: "saved preview",
          tags: ["later"],
          metadataJson: '{"customTitle":true}',
          lastUsedAtMs: 42,
        },
      ],
    ]),
  );
  result.resolve(false);
  await settle();
  expect(out.store!.current.byId.get("initial")).toMatchObject({
    favorite: false,
    title: "saved title",
    textContent: "saved body",
    preview: "saved preview",
    tags: ["later"],
    metadataJson: '{"customTitle":true}',
    lastUsedAtMs: 42,
  });
});

it("does not resurrect a row deleted while a favorite request was pending", async () => {
  const result = deferred<boolean>();
  vi.mocked(service.persistBatchFavorite).mockReturnValueOnce(result.promise);
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkFavorite();
  out.store!.current = removeItems(out.store!.current, new Set(["initial"]));
  result.reject(new Error("save failed"));
  await settle();
  expect(out.store!.current.byId.has("initial")).toBe(false);
  expect(out.store!.current.historyIds).toEqual([]);
});

it("pastes the whole multi-selection at once, selection order preserved", async () => {
  out.store!.current = appendItems(
    out.store!.current,
    [item("a"), item("b"), item("c")],
    "history",
  );
  out.bulk!.selectedIds = new Set(["a", "b", "c"]);
  await out.bulk!.bulkPasteAll();
  expect(vi.mocked(service.pasteClipboardItem).mock.calls.map((c) => c[0].id)).toEqual([
    "a",
    "b",
    "c",
  ]);
  expect([...out.bulk!.selectedIds]).toEqual([]);
});

it("does nothing when there is nothing to paste", async () => {
  out.bulk!.selectedIds = new Set();
  await out.bulk!.bulkPasteAll();
  expect(service.pasteClipboardItem).not.toHaveBeenCalled();
});
it("keeps failed rows selected and reports the failure on a partial bulk paste", async () => {
  out.store!.current = appendItems(
    out.store!.current,
    [item("a"), item("b"), item("c")],
    "history",
  );
  out.bulk!.selectedIds = new Set(["a", "b", "c"]);
  vi.mocked(service.pasteClipboardItem).mockImplementationOnce(async () => {
    throw new Error("paste denied");
  });
  await out.bulk!.bulkPasteAll();
  expect(vi.mocked(service.pasteClipboardItem).mock.calls.map((c) => c[0].id)).toEqual([
    "a",
    "b",
    "c",
  ]);
  expect([...out.bulk!.selectedIds]).toEqual(["a"]);
  expect(showToast).toHaveBeenCalledWith(expect.anything(), "error");
});
it("ignores a bulk paste request while another run is still in flight", async () => {
  out.store!.current = appendItems(out.store!.current, [item("a"), item("b")], "history");
  out.bulk!.selectedIds = new Set(["a", "b"]);
  const gate = deferred<service.PasteOutcome>();
  vi.mocked(service.pasteClipboardItem).mockImplementationOnce(() => gate.promise);
  const first = out.bulk!.bulkPasteAll();
  const second = out.bulk!.bulkPasteAll();
  gate.resolve("pasted");
  await Promise.all([first, second]);
  await settle();
  expect(vi.mocked(service.pasteClipboardItem).mock.calls.map((c) => c[0].id)).toEqual(["a", "b"]);
});
it.each(["pasted", "copied", "failed"] as const)(
  "retains selections added during a pending bulk paste (%s)",
  async (outcome) => {
    const gate = deferred<service.PasteOutcome>();
    vi.mocked(service.pasteClipboardItem).mockImplementationOnce(() => gate.promise);
    out.bulk!.selectedIds = new Set(["initial"]);
    const run = out.bulk!.bulkPasteAll();
    out.store!.current = appendItems(out.store!.current, [item("arrived")], "history");
    out.bulk!.selectedIds = new Set(["initial", "arrived"]);
    gate.resolve(outcome);
    await run;
    expect([...out.bulk!.selectedIds]).toEqual(
      outcome === "pasted" ? ["arrived"] : ["initial", "arrived"],
    );
  },
);
it("retains selections added while a bulk favorite is saving", async () => {
  const gate = deferred<boolean>();
  vi.mocked(service.persistBatchFavorite).mockImplementationOnce(() => gate.promise);
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkFavorite();
  out.store!.current = appendItems(out.store!.current, [item("arrived")], "history");
  out.bulk!.selectedIds = new Set(["initial", "arrived"]);
  gate.resolve(true);
  await settle();
  expect([...out.bulk!.selectedIds]).toEqual(["arrived"]);
  expect(service.persistBatchFavorite).toHaveBeenLastCalledWith(["initial"], true);
});

it("returns to the confirmed favorite after successive bulk saves fail", async () => {
  const first = deferred<boolean>();
  const second = deferred<boolean>();
  vi.mocked(service.persistBatchFavorite)
    .mockReturnValueOnce(first.promise)
    .mockReturnValueOnce(second.promise);
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkFavorite();
  await settle();
  out.bulk!.bulkFavorite();
  expect(out.store!.current.byId.get("initial")!.favorite).toBe(false);
  await settle();
  expect(service.persistBatchFavorite).toHaveBeenCalledTimes(1);
  first.reject(new Error("first failed"));
  await settle();
  expect(service.persistBatchFavorite).toHaveBeenCalledTimes(2);
  second.resolve(false);
  await settle();
  expect(out.store!.current.byId.get("initial")!.favorite).toBe(false);
});

it.each(["bulkPermanentDelete", "bulkDelete"] as const)(
  "%s does not resurrect a canonically removed row on delayed failure",
  async (action) => {
    const gate = deferred<boolean>();
    vi.mocked(service.persistBatchPermanentDelete).mockReturnValueOnce(gate.promise);
    out.store!.current = applyItemPatches(
      out.store!.current,
      new Map([["initial", { deleted: true }]]),
    );
    out.store!.current = setDetailItem(out.store!.current, out.store!.current.byId.get("initial")!);
    out.bulk!.selectedIds = new Set(["initial"]);
    out.bulk![action]();
    expect(out.store!.current.byId.has("initial")).toBe(false);
    out.removeCanonical!(new Set(["initial"]));
    out.store!.current = appendItems(out.store!.current, [item("captured")], "history");
    out.bulk!.selectedIds = new Set(["captured"]);
    gate.resolve(false);
    await settle();
    expect(out.store!.current.historyIds).toEqual(["captured"]);
    expect(out.store!.current.detailId).toBeNull();
    expect(out.history!.deletedHistorySuppressedIds.has("initial")).toBe(true);
    expect([...out.bulk!.selectedIds]).toEqual(["captured"]);
  },
);

it.each(["history", "search"] as const)(
  "accepts same-valued canonical metadata from a %s page before an old save fails",
  async (source) => {
    const gate = deferred<boolean>();
    vi.mocked(service.persistBatchFavorite).mockReturnValueOnce(gate.promise);
    out.bulk!.selectedIds = new Set(["initial"]);
    out.bulk!.bulkFavorite();
    await settle();
    const confirmed = { ...item("initial"), favorite: true };
    if (source === "history") {
      vi.mocked(service.loadClipboardHistory).mockResolvedValueOnce([confirmed]);
      await out.history!.loadActiveHistoryPage();
    } else {
      vi.mocked(service.searchClipboardHistory).mockResolvedValueOnce({
        items: [confirmed],
        totalCount: 1,
        truncated: false,
      });
      out.setQuery!("body");
      await settle(350);
    }
    gate.resolve(false);
    await settle();
    expect(out.store!.current.byId.get("initial")!.favorite).toBe(true);
  },
);

it("keeps a later restore and its paging suppression when an earlier bulk delete succeeds", async () => {
  const gate = deferred<boolean>();
  vi.mocked(service.persistBatchDelete).mockReturnValueOnce(gate.promise);
  vi.mocked(service.loadClipboardHistory).mockReturnValue(new Promise(() => {}));
  vi.mocked(service.loadDeletedClipboardHistory).mockReturnValue(new Promise(() => {}));
  out.bulk!.selectedIds = new Set(["initial"]);
  out.bulk!.bulkDelete();
  expect(out.store!.current.byId.get("initial")!.deleted).toBe(true);
  // Delete has committed but its IPC reply is pending. Another window restores
  // the row; a local restore also keeps stale recycle-bin pages suppressed.
  out.history!.addSuppressedId("initial");
  out.store!.current = applyItemsChangedEvent(out.store!.current, {
    restoredIds: ["initial"],
  });
  gate.resolve(true);
  await settle();
  expect(out.store!.current.byId.get("initial")!.deleted).toBe(false);
  expect(out.history!.deletedHistorySuppressedIds.has("initial")).toBe(true);
});

it("rolls back only failed rows of a partial bulk delete while preserving later restores", async () => {
  const soft = deferred<boolean>();
  const permanent = deferred<boolean>();
  vi.mocked(service.persistBatchDelete).mockReturnValueOnce(soft.promise);
  vi.mocked(service.persistBatchPermanentDelete).mockReturnValueOnce(permanent.promise);
  vi.mocked(service.loadClipboardHistory).mockReturnValue(new Promise(() => {}));
  vi.mocked(service.loadDeletedClipboardHistory).mockReturnValue(new Promise(() => {}));
  out.store!.current = appendItems(
    out.store!.current,
    [{ ...item("bin"), deleted: true }],
    "history",
  );
  out.store!.current = setDetailItem(out.store!.current, out.store!.current.byId.get("initial")!);
  out.bulk!.selectedIds = new Set(["initial", "bin"]);
  out.bulk!.bulkDelete();
  expect(service.persistBatchDelete).toHaveBeenCalledWith(["initial"]);
  expect(service.persistBatchPermanentDelete).toHaveBeenCalledWith(["bin"]);
  expect(out.store!.current.byId.has("bin")).toBe(false);
  out.store!.current = applyItemsChangedEvent(out.store!.current, { restoredIds: ["initial"] });
  out.store!.current = appendItems(out.store!.current, [item("arrived")], "history");
  out.store!.current = setDetailItem(out.store!.current, null);
  out.bulk!.selectedIds = new Set(["arrived"]);
  soft.resolve(true);
  permanent.resolve(false);
  await settle();
  expect(out.store!.current.byId.get("initial")!.deleted).toBe(false);
  expect(out.store!.current.byId.get("bin")!.deleted).toBe(true);
  expect(out.store!.current.byId.has("arrived")).toBe(true);
  expect(out.store!.current.detailId).toBeNull();
  expect([...out.bulk!.selectedIds]).toEqual(["arrived"]);
  expect(showToast).toHaveBeenCalledWith("app.deleteFailed", "error");
});

it.each([true, false])(
  "preserves the actual bulk-delete outcome without later edits (%s)",
  async (ok) => {
    vi.mocked(service.persistBatchDelete).mockResolvedValueOnce(ok);
    vi.mocked(service.persistBatchPermanentDelete).mockResolvedValueOnce(ok);
    vi.mocked(service.loadClipboardHistory).mockReturnValue(new Promise(() => {}));
    vi.mocked(service.loadDeletedClipboardHistory).mockReturnValue(new Promise(() => {}));
    out.store!.current = appendItems(
      out.store!.current,
      [{ ...item("bin"), deleted: true }],
      "history",
    );
    out.bulk!.selectedIds = new Set(["initial", "bin"]);
    out.bulk!.bulkDelete();
    await settle();
    expect(Boolean(out.store!.current.byId.get("initial")!.deleted)).toBe(ok);
    expect(out.store!.current.byId.has("bin")).toBe(!ok);
    expect(showToast).toHaveBeenCalledWith(
      ok ? "toast.bulkDeleteSuccess" : "app.deleteFailed",
      ok ? "success" : "error",
    );
  },
);

it("disposes late native listener registrations exactly once", async () => {
  const scope = createListenerScope(),
    late = deferred<() => void>(),
    early = vi.fn(),
    unlisten = vi.fn();
  scope.add(Promise.resolve(early));
  scope.add(late.promise);
  await settle();
  scope.dispose();
  scope.dispose();
  late.resolve(unlisten);
  await settle();
  expect(early).toHaveBeenCalledTimes(1);
  expect(unlisten).toHaveBeenCalledTimes(1);
});
