import { mount, unmount, flushSync } from "svelte";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import Probe, { type ProbeState } from "./controllers.probe.svelte";
import { createListenerScope } from "./window-lifecycle";
import * as service from "$lib/services/clipboard";
import { appendItems } from "$lib/utils/item-store";
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
