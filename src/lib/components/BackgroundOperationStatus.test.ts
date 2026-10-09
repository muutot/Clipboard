import { mount, unmount, flushSync } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Probe from "./background-operation.probe.svelte";
import { locale } from "$lib/i18n";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;
const running = {
  id: "run-1",
  kind: "sync",
  status: "running",
  phase: "uploading",
  completed: 0,
  total: null,
};
async function settle() {
  for (let i = 0; i < 12; i++) {
    await Promise.resolve();
    flushSync();
  }
}
beforeEach(() => {
  vi.useFakeTimers();
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  invoke.mockReset().mockResolvedValue(running);
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
  vi.useRealTimers();
});
it("recovers a running task after mount and cancels only its reported id", async () => {
  component = mount(Probe, { target });
  await settle();
  expect(target.textContent).toContain("Uploading changes");
  invoke.mockImplementation(async (command) =>
    command === "cancel_background_operation" ? true : { ...running, status: "cancelling" },
  );
  target.querySelector("button")!.click();
  await settle();
  expect(invoke).toHaveBeenCalledWith("cancel_background_operation", { kind: "sync", id: "run-1" });
  expect(target.textContent).toContain("Cancelling");
  expect(target.querySelector("button")!.disabled).toBe(true);
  invoke.mockResolvedValue({ ...running, status: "cancelled" });
  await vi.advanceTimersByTimeAsync(500);
  await settle();
  expect(target.textContent).toContain("Task cancelled");
  expect(target.querySelector("button")).toBeNull();
});
it("stops polling and ignores an in-flight response after unmount", async () => {
  let finish!: (value: unknown) => void;
  invoke.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  component = mount(Probe, { target });
  await settle();
  await unmount(component);
  component = undefined;
  finish(running);
  await settle();
  await vi.advanceTimersByTimeAsync(2000);
  expect(invoke).toHaveBeenCalledTimes(1);
  expect(target.textContent).toBe("");
});
it("reports a polling failure and recovers on the next successful read", async () => {
  invoke.mockRejectedValueOnce(new Error("unavailable"));
  component = mount(Probe, { target });
  await settle();
  expect(target.textContent).toContain("Unable to read task status");
  await vi.advanceTimersByTimeAsync(500);
  await settle();
  expect(target.textContent).toContain("Uploading changes");
  expect(target.textContent).not.toContain("unavailable");
});
