import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, beforeEach, expect, it, vi } from "vitest";
import { locale, t } from "$lib/i18n";
import { confirmDialog, currentConfirm, settleConfirm } from "$lib/services/ui/confirm.svelte";
import Panel from "./StorageLimitsPanel.svelte";
import ConfirmHost from "../common/ConfirmDialog.svelte";

const bridge = vi.hoisted(() => ({ invoke: vi.fn(), feedback: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: bridge.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});

const stats = { itemCount: 3, sizeBytes: 100 };
const result = {
  deletedCount: 3,
  deletedSizeBytes: 100,
  removedFiles: 0,
  searchSync: null,
  warnings: [],
};
function response(command: string) {
  if (command === "get_history_config")
    return { maxItems: 10000, retentionDays: 90, recycleBinDays: 30 };
  if (command === "get_storage_kind_stats") return stats;
  if (command === "permanently_delete_storage_kind") return result;
  throw new Error(`Unexpected fixture command: ${command}`);
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: Error) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
let target: HTMLDivElement;
let hostTarget: HTMLDivElement;
let panel: ReturnType<typeof mount> | undefined;
let host: ReturnType<typeof mount>;
async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
async function closePanel() {
  await unmount(panel!);
  panel = undefined;
}
const statsReads = () =>
  bridge.invoke.mock.calls.filter(([cmd]) => cmd === "get_storage_kind_stats");
const deletes = () =>
  bridge.invoke.mock.calls.filter(([cmd]) => cmd === "permanently_delete_storage_kind");
function deleteText() {
  target.querySelector<HTMLButtonElement>(".danger-action")!.click();
}
beforeAll(() => {
  // jsdom has no native modal API; keep the production confirmation state and host.
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
});
beforeEach(async () => {
  vi.stubGlobal("__TAURI_INTERNALS__", {});
  bridge.invoke.mockReset().mockImplementation(async (command) => response(command));
  bridge.feedback.mockReset();
  settleConfirm(false);
  locale.set("en");
  target = document.createElement("div");
  hostTarget = document.createElement("div");
  document.body.append(target, hostTarget);
  host = mount(ConfirmHost, { target: hostTarget });
  panel = mount(Panel, { target, props: { onfeedback: bridge.feedback } });
  flushSync();
  await settle();
  expect(target.querySelector<HTMLButtonElement>(".danger-action")!.disabled).toBe(false);
});
afterEach(async () => {
  settleConfirm(false);
  await settle();
  if (panel) await closePanel();
  await unmount(host);
  target.remove();
  hostTarget.remove();
  vi.restoreAllMocks();
  vi.unstubAllGlobals();
});

it("does not replace a newer confirmation when old statistics arrive after panel teardown", async () => {
  const pending = deferred<typeof stats>();
  bridge.invoke.mockImplementation((command) =>
    command === "get_storage_kind_stats" ? pending.promise : Promise.resolve(response(command)),
  );
  deleteText();
  await settle();
  expect(statsReads()).toHaveLength(5);
  expect(currentConfirm()).toBeNull();
  await closePanel();
  let outcome: boolean | undefined;
  const newer = confirmDialog({ title: "Newer request", message: "Keep this current" });
  void newer.then((answer) => {
    outcome = answer;
  });
  pending.resolve(stats);
  await settle();
  expect(hostTarget.querySelector(".confirm-title")!.textContent).toBe("Newer request");
  expect(outcome).toBeUndefined();
  expect(deletes()).toHaveLength(0);
  settleConfirm(false);
  await expect(newer).resolves.toBe(false);
});

it("cancels its pending confirmation when the panel is torn down", async () => {
  deleteText();
  await settle();
  expect(currentConfirm()?.danger).toBe(true);
  await closePanel();
  await settle();
  expect(currentConfirm()).toBeNull();
  expect(hostTarget.querySelector(".confirm-dialog")).toBeNull();
  expect(deletes()).toHaveLength(0);
});

it("retains another caller's newer confirmation when the panel is torn down", async () => {
  deleteText();
  await settle();
  expect(currentConfirm()?.danger).toBe(true);
  const newer = confirmDialog({ title: "Newer request", message: "Keep this current" });
  await closePanel();
  await settle();
  expect(hostTarget.querySelector(".confirm-title")!.textContent).toBe("Newer request");
  expect(deletes()).toHaveLength(0);
  settleConfirm(false);
  await expect(newer).resolves.toBe(false);
});

it("does not submit deletion when teardown precedes the confirmation continuation", async () => {
  deleteText();
  await settle();
  hostTarget.querySelector<HTMLButtonElement>(".confirm-accept")!.click();
  await closePanel();
  await settle();
  expect(deletes()).toHaveLength(0);
  expect(bridge.feedback).not.toHaveBeenCalled();
});

it("does not refetch or publish a late statistics error after panel teardown", async () => {
  const pending = deferred<typeof stats>();
  bridge.invoke.mockImplementation((command) =>
    command === "get_storage_kind_stats" ? pending.promise : Promise.resolve(response(command)),
  );
  deleteText();
  await settle();
  await closePanel();
  pending.reject(new Error("late fixture statistics failure"));
  await settle();
  expect(statsReads()).toHaveLength(5);
  expect(bridge.feedback).not.toHaveBeenCalled();
  expect(currentConfirm()).toBeNull();
});

it.each(["succeeded", "failed"])(
  "does not refetch or publish an already submitted delete that %s after teardown",
  async (outcome) => {
    const pending = deferred<typeof result>();
    bridge.invoke.mockImplementation((command) =>
      command === "permanently_delete_storage_kind"
        ? pending.promise
        : Promise.resolve(response(command)),
    );
    deleteText();
    await settle();
    hostTarget.querySelector<HTMLButtonElement>(".confirm-accept")!.click();
    await settle();
    expect(deletes()).toEqual([
      ["permanently_delete_storage_kind", { kind: "text", expected: stats }],
    ]);
    await closePanel();
    if (outcome === "succeeded") pending.resolve(result);
    else pending.reject(new Error("late fixture deletion failure"));
    await settle();
    expect(statsReads()).toHaveLength(5);
    expect(bridge.feedback).not.toHaveBeenCalled();
  },
);

it("keeps cancellation retryable and submits the confirmed count only while mounted", async () => {
  deleteText();
  await settle();
  hostTarget.querySelector<HTMLButtonElement>(".confirm-cancel")!.click();
  await settle();
  expect(deletes()).toHaveLength(0);
  deleteText();
  await settle();
  hostTarget.querySelector<HTMLButtonElement>(".confirm-accept")!.click();
  await settle();
  expect(deletes()).toEqual([
    ["permanently_delete_storage_kind", { kind: "text", expected: stats }],
  ]);
  expect(bridge.feedback).toHaveBeenCalledWith(expect.any(String), true);
  expect(currentConfirm()).toBeNull();
  expect(target.querySelector<HTMLButtonElement>(".danger-action")!.disabled).toBe(false);
});

it("reports a mounted deletion failure and keeps the action available for retry", async () => {
  bridge.invoke.mockImplementation(async (command) => {
    if (command === "permanently_delete_storage_kind") throw new Error("fixture deletion failure");
    return response(command);
  });
  deleteText();
  await settle();
  hostTarget.querySelector<HTMLButtonElement>(".confirm-accept")!.click();
  await settle();
  expect(bridge.feedback).toHaveBeenCalledWith(
    expect.stringContaining("fixture deletion failure"),
    false,
  );
  expect(target.querySelector<HTMLButtonElement>(".danger-action")!.disabled).toBe(false);
  expect(currentConfirm()).toBeNull();
  expect(target.textContent).toContain(t("storage.deleteByKindTitle"));
});
