import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import { invoke } from "@tauri-apps/api/core";
import { emit, listen } from "@tauri-apps/api/event";
import TagManagementSettingsPanel from "./TagManagementSettingsPanel.svelte";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(), listen: vi.fn() }));

let host: HTMLDivElement;
let app: ReturnType<typeof mount> | null;
const runtimeDescriptor = Object.getOwnPropertyDescriptor(window, "__TAURI_INTERNALS__");
const alpha = { name: "alpha", count: 1, color: "#5c7cfa" };

async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function rename() {
  const input = host.querySelector<HTMLInputElement>(".tag-name-input")!;
  input.value = "beta";
  input.dispatchEvent(new Event("blur"));
  flushSync();
}
function remove() {
  host.querySelector<HTMLButtonElement>(".tag-delete")!.click();
  flushSync();
  host.querySelector<HTMLButtonElement>(".tag-delete")!.click();
  flushSync();
}
function color() {
  host.querySelector<HTMLButtonElement>(".tag-color-trigger")!.click();
  flushSync();
  host.querySelector<HTMLButtonElement>('.tag-swatch-option[aria-label="#e5484d"]')!.click();
  flushSync();
}
const actions = { rename_tag: rename, delete_tag: remove, set_tag_color: color };

function render() {
  app = mount(TagManagementSettingsPanel, {
    target: host,
    props: { onclose: vi.fn(), showHeader: false },
  });
  flushSync();
}
function changed() {
  vi.mocked(listen).mock.calls.at(-1)![1]({ event: "tags-changed", id: 1, payload: {} });
  flushSync();
}

beforeEach(async () => {
  vi.resetAllMocks();
  locale.set("en");
  Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {} });
  vi.mocked(emit).mockResolvedValue();
  vi.mocked(listen).mockResolvedValue(vi.fn());
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "list_all_tags") return [{ ...alpha }];
    return command === "set_tag_color" ? true : 1;
  });
  host = document.createElement("div");
  document.body.append(host);
  render();
  await settle();
});
afterEach(async () => {
  if (app) await unmount(app);
  app = null;
  host.remove();
  vi.restoreAllMocks();
  if (runtimeDescriptor) Object.defineProperty(window, "__TAURI_INTERNALS__", runtimeDescriptor);
  else Reflect.deleteProperty(window, "__TAURI_INTERNALS__");
});

it.each([
  ["rename_tag", null],
  ["delete_tag", null],
  ["set_tag_color", null],
  ["set_tag_color", false],
] as const)("reports an unsuccessful %s result (%s) and allows retry", async (command, result) => {
  vi.mocked(invoke).mockResolvedValueOnce(result);
  actions[command]();
  await settle();
  expect(host.querySelector('[role="status"]')?.textContent).toBe("Save failed");
  expect(host.querySelector('[role="status"]')?.classList.contains("success")).toBe(false);
  expect(emit).not.toHaveBeenCalled();
  expect(host.querySelector<HTMLInputElement>(".tag-name-input")?.value).toBe("alpha");
  expect(host.querySelector(".tag-color-trigger")?.getAttribute("style")).toContain(alpha.color);
  if (command === "delete_tag") {
    // Confirmation stays available after a failed delete.
    host.querySelector<HTMLButtonElement>(".tag-delete")!.click();
  } else actions[command]();
  await settle();
  expect(emit).toHaveBeenCalledTimes(1);
});

it.each(["rename_tag", "delete_tag", "set_tag_color"] as const)(
  "handles rejected %s IPC without a success announcement",
  async (command) => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("synthetic database busy"));
    actions[command]();
    await settle();
    expect(host.querySelector('[role="status"]')?.textContent).toBe("Save failed");
    expect(emit).not.toHaveBeenCalled();
    expect(host.querySelector<HTMLInputElement>(".tag-name-input")?.value).toBe("alpha");
  },
);

it.each(["rename_tag", "delete_tag"] as const)(
  "accepts zero affected records from a successful %s registry mutation",
  async (command) => {
    vi.mocked(invoke).mockResolvedValueOnce(0);
    actions[command]();
    await settle();
    expect(host.querySelector('[role="status"]')?.classList.contains("success")).toBe(true);
    expect(emit).toHaveBeenCalledWith(
      "tags-changed",
      command === "rename_tag" ? { renamed: { old: "alpha", new: "beta" } } : { deleted: "alpha" },
    );
  },
);

it("does not schedule late failure feedback after the panel is destroyed", async () => {
  let resolve!: (value: unknown) => void;
  vi.mocked(invoke).mockReturnValueOnce(new Promise((yes) => (resolve = yes)));
  color();
  await unmount(app!);
  app = null;
  const timer = vi.spyOn(globalThis, "setTimeout");
  resolve(false);
  await settle();
  expect(timer).not.toHaveBeenCalled();
  expect(emit).not.toHaveBeenCalled();
});

it.each([
  ["initial", "null"],
  ["initial", "rejection"],
  ["refresh", "null"],
  ["refresh", "rejection"],
] as const)("offers retry after a failed %s tag list (%s)", async (phase, failure) => {
  if (phase === "initial") {
    await unmount(app!);
    app = null;
  }
  if (failure === "null") vi.mocked(invoke).mockResolvedValueOnce(null);
  else vi.mocked(invoke).mockRejectedValueOnce(new Error("synthetic list read failure"));
  if (phase === "initial") render();
  else changed();
  await settle();
  expect(host.querySelector(".settings-state")?.textContent).toContain("Could not load tags.");
  expect(host.querySelector(".empty-state")).toBeNull();
  expect(host.querySelector(".tag-name-input")).toBeNull();
  host.querySelector<HTMLButtonElement>(".settings-state button")!.click();
  await settle();
  expect(host.querySelector(".settings-state")).toBeNull();
  expect(host.querySelector<HTMLInputElement>(".tag-name-input")?.value).toBe("alpha");
});

it("distinguishes a successfully loaded empty tag list from a failed read", async () => {
  vi.mocked(invoke).mockResolvedValueOnce([]);
  changed();
  await settle();
  expect(host.querySelector(".empty-state")).not.toBeNull();
  expect(host.querySelector(".settings-state button")).toBeNull();
});
