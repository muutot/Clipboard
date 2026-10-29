import { beforeEach, describe, expect, it, vi } from "vitest";
import { get } from "svelte/store";

const bridge = vi.hoisted(() => ({ invoke: vi.fn() }));
vi.mock("$lib/services/runtime", () => ({ invokeTauri: bridge.invoke }));

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

beforeEach(() => {
  vi.resetModules();
  bridge.invoke.mockReset();
  bridge.invoke.mockImplementation(async (name: string) => {
    if (name === "get_window_config")
      return { launchAtStartup: false, closeToTray: true, singleInstance: true };
  });
});

describe("window config persistence", () => {
  it("keeps a later successful edit when an earlier different field fails", async () => {
    const { windowConfig } = await import("./window");
    await windowConfig.ensureLoaded();
    const failed = deferred<void>();
    bridge.invoke.mockImplementation((name: string, args: Record<string, unknown>) => {
      if (name === "set_window_config" && args.launchAtStartup === true) return failed.promise;
      return Promise.resolve();
    });
    const first = windowConfig.update({ launchAtStartup: true });
    const firstFailure = expect(first).rejects.toThrow("save denied");
    const second = windowConfig.update({ closeToTray: false });
    failed.reject(new Error("save denied"));
    await firstFailure;
    await second;
    expect(get(windowConfig)).toEqual({
      launchAtStartup: false,
      closeToTray: false,
      singleInstance: true,
    });
  });

  it("keeps a newer successful reaffirmation of the same value", async () => {
    const { windowConfig } = await import("./window");
    await windowConfig.ensureLoaded();
    const failed = deferred<void>();
    let writes = 0;
    bridge.invoke.mockImplementation(() => (++writes === 1 ? failed.promise : Promise.resolve()));
    const first = windowConfig.update({ launchAtStartup: true });
    const firstFailure = expect(first).rejects.toThrow("save denied");
    const second = windowConfig.update({ launchAtStartup: true });
    failed.reject(new Error("save denied"));
    await firstFailure;
    await second;
    expect(get(windowConfig).launchAtStartup).toBe(true);
  });

  it("restores the last confirmed value when successive edits both fail", async () => {
    const { windowConfig } = await import("./window");
    await windowConfig.ensureLoaded();
    bridge.invoke.mockRejectedValue(new Error("save denied"));
    const results = await Promise.allSettled([
      windowConfig.update({ launchAtStartup: true }),
      windowConfig.update({ launchAtStartup: false }),
    ]);
    expect(results.every((result) => result.status === "rejected")).toBe(true);
    expect(get(windowConfig).launchAtStartup).toBe(false);
  });

  it("retains an edit during hydration and waits for the backend baseline before saving", async () => {
    const loaded = deferred<{
      launchAtStartup: boolean;
      closeToTray: boolean;
      singleInstance: boolean;
    }>();
    bridge.invoke.mockImplementation((name: string) =>
      name === "get_window_config" ? loaded.promise : Promise.resolve(),
    );
    const { windowConfig } = await import("./window");
    const update = windowConfig.update({ launchAtStartup: true });
    expect(get(windowConfig).launchAtStartup).toBe(true);
    loaded.resolve({ launchAtStartup: false, closeToTray: false, singleInstance: true });
    await update;
    await windowConfig.ensureLoaded();
    expect(get(windowConfig)).toEqual({
      launchAtStartup: true,
      closeToTray: false,
      singleInstance: true,
    });
  });
});
