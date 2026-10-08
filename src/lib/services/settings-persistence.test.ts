import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";
import { applySettingsPatch, type SettingsPatch } from "$lib/utils/settings-patch";

const bridge = vi.hoisted(() => ({
  invoke: vi.fn(),
  listeners: new Set<(event: { payload: unknown }) => void>(),
}));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async (_name, callback) => {
    bridge.listeners.add(callback);
    return () => bridge.listeners.delete(callback);
  }),
}));
vi.mock("$lib/i18n", () => ({ setLocale: vi.fn() }));

type Store = typeof import("./settings").generalSettings;
let stores: Store[];
let server: SettingsPatch;
let legacy: boolean;
let beforeWrite: (() => Promise<void>) | undefined;
let beforeRead: (() => Promise<void>) | undefined;
const copy = <T>(value: T): T => JSON.parse(JSON.stringify(value));
const broadcast = () => bridge.listeners.forEach((listener) => listener({ payload: copy(server) }));
const writes = () => bridge.invoke.mock.calls.filter(([name]) => name === "set_general_settings");

it("persists saved search state and hydrates another window", async () => {
  const first = await loadedStore();
  const second = await loadedStore();
  const saved = [
    {
      id: "one",
      name: "Work",
      query: "invoice",
      activeFilter: "image" as const,
      tagFilter: "work",
      sourceAppFilter: "Editor",
      dateFilter: "week" as const,
      sortRules: [{ field: "size" as const, direction: "asc" as const }],
    },
  ];
  first.updateSetting("savedSearches", saved);
  await first.flush();
  expect(server.savedSearches).toEqual(saved);
  expect(get(second).savedSearches).toEqual(saved);
  const reopened = await loadedStore();
  expect(get(reopened).savedSearches).toEqual(saved);
});
function deferred() {
  let resolve!: () => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<void>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function createStore() {
  vi.resetModules();
  const { generalSettings } = await import("./settings");
  stores.push(generalSettings);
  return generalSettings;
}
async function loadedStore() {
  const store = await createStore();
  await store.initialize();
  return store;
}

beforeEach(() => {
  vi.useFakeTimers();
  window.localStorage.clear();
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  stores = [];
  server = {};
  legacy = false;
  beforeWrite = undefined;
  beforeRead = undefined;
  bridge.listeners.clear();
  bridge.invoke.mockReset().mockImplementation(async (command, args) => {
    if (command === "get_window_config")
      return { launchAtStartup: false, closeToTray: true, singleInstance: true };
    if (command === "get_general_settings") {
      const response = { settings: copy(server), legacyMigrationRequired: legacy };
      await beforeRead?.();
      return response;
    }
    if (command === "set_general_settings") {
      await beforeWrite?.();
      server = args.patch ? applySettingsPatch(server, args.patch) : copy(args.settings);
      legacy = false;
      broadcast();
      return copy(server);
    }
    throw new Error(`unexpected command: ${command}`);
  });
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(async () => {
  beforeRead = undefined;
  beforeWrite = undefined;
  for (const store of stores) {
    await store.flush();
    store.destroy();
  }
  Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("desktop settings persistence", () => {
  it("keeps newer edits when an earlier write fails, then retries both fields", async () => {
    const store = await loadedStore();
    const gate = deferred();
    beforeWrite = () => gate.promise;
    store.updateSetting("pageSizeLimit", 600);
    const first = store.flush();
    const failure = expect(first).rejects.toThrow("disk full");
    store.merge({ pageSizeLimit: 700, searchCacheSize: 1000 });
    gate.reject(new Error("disk full"));
    await failure;
    expect(get(store).pageSizeLimit).toBe(700);
    beforeWrite = undefined;
    await store.flush();
    expect(server).toMatchObject({ pageSizeLimit: 700, searchCacheSize: 1000 });
    expect(writes()[1][1]).toEqual({ patch: { pageSizeLimit: 700, searchCacheSize: 1000 } });
  });

  it("merges simultaneous edits from two windows down to nested fields", async () => {
    const a = await loadedStore();
    const b = await loadedStore();
    a.merge({ searchCacheSize: 1000, display: { ...get(a).display, maxTextLines: 6 } });
    b.merge({ pageSizeLimit: 800, display: { ...get(b).display, pageSize: 200 } });
    await Promise.all([a.flush(), b.flush()]);
    expect(server).toMatchObject({
      searchCacheSize: 1000,
      pageSizeLimit: 800,
      display: { maxTextLines: 6, pageSize: 200 },
    });
    for (const store of [a, b]) {
      expect(get(store)).toMatchObject(server);
    }
    expect(writes()[0][1]).toEqual({
      patch: { searchCacheSize: 1000, display: { maxTextLines: 6 } },
    });
    expect(writes()[1][1]).toEqual({ patch: { pageSizeLimit: 800, display: { pageSize: 200 } } });
  });

  it("flush includes an edit arriving during the final backend refresh", async () => {
    const store = await loadedStore();
    const gate = deferred();
    let refreshing = false;
    beforeRead = () => {
      refreshing = true;
      return gate.promise;
    };
    store.updateSetting("pageSizeLimit", 800);
    const flush = store.flush();
    for (let i = 0; i < 20 && !refreshing; i++) await Promise.resolve();
    expect(refreshing).toBe(true);
    store.updateSetting("searchCacheSize", 1000);
    await vi.advanceTimersByTimeAsync(120);
    beforeRead = undefined;
    gate.resolve();
    await flush;
    expect(server).toMatchObject({ pageSizeLimit: 800, searchCacheSize: 1000 });
    expect(get(store).searchCacheSize).toBe(1000);
  });

  it("clears optional fields and preserves array replacement through IPC", async () => {
    server = { activePresetId: "preset", iconColors: { text: "#ffffff", file: "#000000" } };
    const store = await loadedStore();
    store.merge({
      activePresetId: undefined,
      iconColors: { file: "#000000" },
      searchSortRules: [{ field: "title", direction: "asc" }],
    });
    await store.flush();
    expect(server).not.toHaveProperty("activePresetId");
    expect(get(store).activePresetId).toBeUndefined();
    expect(server.iconColors).toEqual({ file: "#000000" });
    expect(server.searchSortRules).toEqual([{ field: "title", direction: "asc" }]);
    expect(writes()[0][1].patch).toMatchObject({
      activePresetId: null,
      iconColors: { text: null },
    });
  });

  it("preserves remote nested values when a local edit precedes hydration", async () => {
    server = { display: { pageSize: 200 } };
    const gate = deferred();
    beforeRead = () => gate.promise;
    const store = await createStore();
    store.updateSetting("display", { ...get(store).display, maxTextLines: 6 });
    beforeRead = undefined;
    gate.resolve();
    await store.initialize();
    await store.flush();
    expect(server.display).toEqual({ pageSize: 200, maxTextLines: 6 });
    expect(get(store).display.pageSize).toBe(200);
  });

  it("keeps migration keys after failure and preserves edits made during migration", async () => {
    legacy = true;
    server = { display: { pageSize: 200 } };
    window.localStorage.setItem(
      "generalSettings",
      JSON.stringify({ display: { maxTextLines: 6 } }),
    );
    const gate = deferred();
    beforeWrite = () => gate.promise;
    const store = await createStore();
    for (let i = 0; i < 20 && !writes().length; i++) await Promise.resolve();
    expect(writes()).toHaveLength(1);
    store.updateSetting("pageSizeLimit", 800);
    gate.reject(new Error("disk full"));
    await store.initialize();
    expect(window.localStorage.getItem("generalSettings")).not.toBeNull();
    beforeWrite = undefined;
    await store.flush();
    expect(server).toMatchObject({
      pageSizeLimit: 800,
      display: { pageSize: 200, maxTextLines: 6 },
    });
    expect(window.localStorage.getItem("generalSettings")).toBeNull();
  });
});
