import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import type { UnlistenFn } from "@tauri-apps/api/event";
import type { SourceMappingRule } from "$lib/services/clipboard";
import { createSourceMappingStore, type SourceMappingStore } from "./source-mapping.svelte";
import Probe from "./source-mapping.probe.svelte";

const bridge = vi.hoisted(() => ({
  getRules: vi.fn(),
  listen: vi.fn(),
  listeners: new Set<() => void>(),
}));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/clipboard", () => ({ getSourceMappingRules: bridge.getRules }));
vi.mock("@tauri-apps/api/event", () => ({
  listen: bridge.listen,
}));

type WindowStore = typeof import("./source-mapping.svelte");
let windows: WindowStore[];
let stores: SourceMappingStore[];
let server: SourceMappingRule[];
const capture = { sourceApp: "Chrome", webSource: { domain: "example.com", title: "Page" } };
const rulesFor = (name: string): SourceMappingRule[] => [
  { domain: "example.com", titleKeyword: "", name },
];
const broadcast = () => bridge.listeners.forEach((listener) => listener());

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => (resolve = yes));
  return { promise, resolve };
}

function createStore() {
  const store = createSourceMappingStore();
  stores.push(store);
  return store;
}

async function settle() {
  for (let i = 0; i < 15; i++) {
    await Promise.resolve();
    flushSync();
  }
}

async function loadWindow() {
  // Separate module instances model the separate JS realms of WebviewWindows.
  vi.resetModules();
  const store = await import("./source-mapping.svelte");
  windows.push(store);
  await store.ensureSourceMappingRules();
  await settle();
  return store;
}

beforeEach(() => {
  windows = [];
  stores = [];
  server = rulesFor("Before");
  bridge.listeners.clear();
  bridge.listen.mockReset().mockImplementation(async (name, callback) => {
    expect(name).toBe("source-mapping-rules-changed");
    bridge.listeners.add(callback);
    return () => bridge.listeners.delete(callback);
  });
  bridge.getRules.mockReset().mockImplementation(async () => server.map((rule) => ({ ...rule })));
  vi.spyOn(console, "error").mockImplementation(() => {});
});
afterEach(() => {
  for (const store of windows) store.destroySourceMappingRules();
  for (const store of stores) store.destroy();
  bridge.listeners.clear();
  vi.restoreAllMocks();
});

it("refreshes every independent window cache after a saved-rule notification", async () => {
  const first = await loadWindow();
  const second = await loadWindow();
  expect(first.resolveWebSourceName(capture)).toBe("Before");
  expect(second.resolveWebSourceName(capture)).toBe("Before");

  server = rulesFor("After");
  broadcast();
  await settle();

  expect(first.resolveWebSourceName(capture)).toBe("After");
  expect(second.resolveWebSourceName(capture)).toBe("After");
});

it("registers once before hydration and coalesces concurrent initialization", async () => {
  const store = createStore();
  bridge.getRules.mockImplementation(async () => {
    expect(bridge.listeners.size).toBe(1);
    return server;
  });
  await Promise.all([store.initialize(), store.initialize()]);
  await store.initialize();
  expect(bridge.listen).toHaveBeenCalledTimes(1);
  expect(bridge.getRules).toHaveBeenCalledTimes(1);
  expect(store.resolveWebSourceName(capture)).toBe("Before");
});

it("discards an initial snapshot invalidated by a notification during the read", async () => {
  const oldRead = deferred<SourceMappingRule[]>();
  bridge.getRules.mockReturnValueOnce(oldRead.promise);
  const store = createStore();
  const loading = store.initialize();
  await settle();
  expect(bridge.listeners.size).toBe(1);
  server = rulesFor("After");
  broadcast();
  oldRead.resolve(rulesFor("Before"));
  await loading;
  expect(bridge.getRules).toHaveBeenCalledTimes(2);
  expect(store.resolveWebSourceName(capture)).toBe("After");
});

it("settles an explicit refresh on the latest rules when hydration is in flight", async () => {
  const oldRead = deferred<SourceMappingRule[]>();
  bridge.getRules.mockReturnValueOnce(oldRead.promise);
  const store = createStore();
  const loading = store.initialize();
  await settle();
  server = rulesFor("After");
  const refreshing = store.refresh();
  await settle();
  oldRead.resolve(rulesFor("Before"));
  await Promise.all([loading, refreshing]);
  expect(store.resolveWebSourceName(capture)).toBe("After");
});

it("retries a failed initial read without leaking another listener", async () => {
  bridge.getRules.mockRejectedValueOnce(new Error("read unavailable"));
  const store = createStore();
  await store.initialize();
  expect(store.resolveWebSourceName(capture)).toBe("Page");
  await store.initialize();
  expect(store.resolveWebSourceName(capture)).toBe("Before");
  expect(bridge.listen).toHaveBeenCalledTimes(1);
  expect(bridge.getRules).toHaveBeenCalledTimes(2);
});

it("retries a failed registration while retaining the successfully loaded rules", async () => {
  bridge.listen.mockRejectedValueOnce(new Error("listener unavailable"));
  const store = createStore();
  await store.initialize();
  expect(store.resolveWebSourceName(capture)).toBe("Before");
  expect(bridge.listeners.size).toBe(0);
  server = rulesFor("After");
  await store.initialize();
  expect(bridge.listeners.size).toBe(1);
  expect(bridge.getRules).toHaveBeenCalledTimes(2);
  expect(store.resolveWebSourceName(capture)).toBe("After");
  server = rulesFor("Newest");
  broadcast();
  await settle();
  expect(store.resolveWebSourceName(capture)).toBe("Newest");
});

it("retains the last good names after a failed event read and retries on the next event", async () => {
  const store = createStore();
  await store.initialize();
  server = rulesFor("After");
  bridge.getRules.mockRejectedValueOnce(new Error("read unavailable"));
  broadcast();
  await settle();
  expect(store.resolveWebSourceName(capture)).toBe("Before");
  broadcast();
  await settle();
  expect(store.resolveWebSourceName(capture)).toBe("After");
});

it("releases a listener whose registration completes after destruction", async () => {
  const registration = deferred<UnlistenFn>();
  let callback!: () => void;
  bridge.listen.mockImplementationOnce((_name, listener) => {
    callback = listener;
    return registration.promise;
  });
  const store = createStore();
  const loading = store.initialize();
  await settle();
  store.destroy();
  bridge.listeners.add(callback);
  registration.resolve(() => {
    bridge.listeners.delete(callback);
  });
  await loading;
  callback();
  await settle();
  expect(bridge.listeners.size).toBe(0);
  expect(bridge.getRules).not.toHaveBeenCalled();
});

it("ignores a destroyed lifecycle's read after the store is reinitialized", async () => {
  const oldRead = deferred<SourceMappingRule[]>();
  bridge.getRules.mockReturnValueOnce(oldRead.promise);
  const store = createStore();
  const oldLoading = store.initialize();
  await settle();
  store.destroy();
  expect(bridge.listeners.size).toBe(0);
  server = rulesFor("After");
  await store.initialize();
  oldRead.resolve(rulesFor("Before"));
  await oldLoading;
  expect(store.resolveWebSourceName(capture)).toBe("After");
  expect(bridge.listeners.size).toBe(1);
});

it("updates a mounted derived display name after a remote save", async () => {
  const store = createStore();
  await store.initialize();
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(Probe, { target, props: { store, item: capture } });
  try {
    flushSync();
    expect(target.textContent).toBe("Before");
    server = rulesFor("After");
    broadcast();
    await settle();
    expect(target.textContent).toBe("After");
  } finally {
    await unmount(component);
    target.remove();
  }
});
