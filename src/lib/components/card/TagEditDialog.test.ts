import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { locale } from "$lib/i18n";
import { listAllTags, renameTag, setTagColor } from "$lib/services/clipboard";
import { showToast } from "$lib/services/toast";
import { emit } from "@tauri-apps/api/event";
import TagEditDialog from "./TagEditDialog.svelte";

vi.mock("$lib/services/clipboard", () => ({
  listAllTags: vi.fn(),
  renameTag: vi.fn(),
  setTagColor: vi.fn(),
}));
vi.mock("$lib/services/toast", () => ({ showToast: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn(async () => {}) }));

let host: HTMLDivElement;
let app: ReturnType<typeof mount> | null;
const showModalDescriptor = Object.getOwnPropertyDescriptor(
  HTMLDialogElement.prototype,
  "showModal",
);

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function input() {
  return host.querySelector<HTMLInputElement>(".tag-edit-name")!;
}
function draft(value: string) {
  input().value = value;
  input().dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}
function rename(value: string) {
  draft(value);
  input().dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
  flushSync();
}
function color() {
  host.querySelector<HTMLButtonElement>('.tag-swatch-option[aria-label="#e5484d"]')!.click();
  flushSync();
}
async function destroy() {
  if (app) await unmount(app);
  app = null;
}

beforeEach(() => {
  vi.resetAllMocks();
  locale.set("en");
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
    configurable: true,
    value: vi.fn(),
  });
  vi.mocked(listAllTags).mockResolvedValue([{ name: "alpha", count: 1, color: "" }]);
  vi.mocked(renameTag).mockResolvedValue(1);
  vi.mocked(setTagColor).mockResolvedValue(true);
  host = document.createElement("div");
  document.body.append(host);
  app = mount(TagEditDialog, {
    target: host,
    props: { tag: "alpha", color: "", onclose: vi.fn() },
  });
  flushSync();
});
afterEach(async () => {
  await destroy();
  host.remove();
  vi.restoreAllMocks();
  if (showModalDescriptor)
    Object.defineProperty(HTMLDialogElement.prototype, "showModal", showModalDescriptor);
  else Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
});

it("preserves a later name draft when an earlier rename completes", async () => {
  const gate = deferred<number>();
  vi.mocked(renameTag).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  draft("gamma");
  gate.resolve(1);
  await settle();
  expect(input().value).toBe("gamma");
  expect(emit).toHaveBeenCalledWith("tags-changed", { renamed: { old: "alpha", new: "beta" } });
});

it("renames the last confirmed name in submission order", async () => {
  const gate = deferred<number>();
  vi.mocked(renameTag).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  rename("gamma");
  await settle();
  expect(renameTag).toHaveBeenCalledTimes(1);
  expect(renameTag).toHaveBeenCalledWith("alpha", "beta");
  gate.resolve(1);
  await settle();
  expect(vi.mocked(renameTag).mock.calls).toEqual([
    ["alpha", "beta"],
    ["beta", "gamma"],
  ]);
  expect(vi.mocked(emit).mock.calls).toEqual([
    ["tags-changed", { renamed: { old: "alpha", new: "beta" } }],
    ["tags-changed", { renamed: { old: "beta", new: "gamma" } }],
  ]);
  expect(input().value).toBe("gamma");
});

it("saves a color under the confirmed name after a pending rename", async () => {
  const gate = deferred<number>();
  vi.mocked(renameTag).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  color();
  await settle();
  expect(setTagColor).not.toHaveBeenCalled();
  gate.resolve(1);
  await settle();
  expect(setTagColor).toHaveBeenCalledWith("beta", "#e5484d");
});

it("does not start a rename after its preflight finishes on a closed dialog", async () => {
  const gate = deferred<Awaited<ReturnType<typeof listAllTags>>>();
  vi.mocked(listAllTags).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  await destroy();
  gate.resolve([]);
  await settle();
  expect(renameTag).not.toHaveBeenCalled();
  expect(emit).not.toHaveBeenCalled();
});

it("announces an already committed rename after unmount but skips queued edits", async () => {
  const gate = deferred<number>();
  vi.mocked(renameTag).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  rename("gamma");
  color();
  await destroy();
  gate.resolve(1);
  await settle();
  expect(renameTag).toHaveBeenCalledTimes(1);
  expect(setTagColor).not.toHaveBeenCalled();
  expect(emit).toHaveBeenCalledWith("tags-changed", { renamed: { old: "alpha", new: "beta" } });
  expect(showToast).not.toHaveBeenCalled();
});

it("allows zero affected records when the backend renames only a registry color", async () => {
  vi.mocked(renameTag).mockResolvedValueOnce(0);
  rename("beta");
  await settle();
  expect(input().value).toBe("beta");
  expect(emit).toHaveBeenCalledWith("tags-changed", { renamed: { old: "alpha", new: "beta" } });
});

it("checks conflicts and leaves the confirmed name available for retry", async () => {
  vi.mocked(listAllTags).mockResolvedValueOnce([{ name: "beta", count: 1, color: "" }]);
  rename("beta");
  await settle();
  expect(renameTag).not.toHaveBeenCalled();
  expect(showToast).toHaveBeenCalledWith("A tag with this name already exists", "error");
  rename("gamma");
  await settle();
  expect(renameTag).toHaveBeenCalledWith("alpha", "gamma");
});

it("retains the confirmed source after a failed rename before a queued retry", async () => {
  const gate = deferred<number>();
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.mocked(renameTag).mockReturnValueOnce(gate.promise);
  rename("beta");
  await settle();
  rename("gamma");
  gate.reject(new Error("synthetic rename failure"));
  await settle();
  expect(vi.mocked(renameTag).mock.calls).toEqual([
    ["alpha", "beta"],
    ["alpha", "gamma"],
  ]);
  expect(input().value).toBe("gamma");
  expect(emit).toHaveBeenCalledExactlyOnceWith("tags-changed", {
    renamed: { old: "alpha", new: "gamma" },
  });
});

it("finishes an earlier color write before renaming its registry key", async () => {
  const gate = deferred<boolean>();
  vi.mocked(setTagColor).mockReturnValueOnce(gate.promise);
  color();
  await settle();
  rename("beta");
  await settle();
  expect(renameTag).not.toHaveBeenCalled();
  gate.resolve(true);
  await settle();
  expect(setTagColor).toHaveBeenCalledWith("alpha", "#e5484d");
  expect(renameTag).toHaveBeenCalledWith("alpha", "beta");
  expect(input().value).toBe("beta");
});

it.each(["rejected", "false", "null"])(
  "reports a %s color save and keeps the current swatch available for retry",
  async (result) => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    if (result === "rejected")
      vi.mocked(setTagColor).mockRejectedValueOnce(new Error("synthetic color failure"));
    else vi.mocked(setTagColor).mockResolvedValueOnce(result === "false" ? false : null);
    color();
    await settle();
    expect(showToast).toHaveBeenCalledExactlyOnceWith("Save failed", "error");
    expect(emit).not.toHaveBeenCalled();
    expect(host.querySelector(".tag-swatch-option.active")).toBeNull();
    color();
    await settle();
    expect(setTagColor).toHaveBeenCalledTimes(2);
    expect(emit).toHaveBeenCalledExactlyOnceWith("tags-changed", {});
    expect(host.querySelector(".tag-swatch-option.active")?.getAttribute("aria-label")).toBe(
      "#e5484d",
    );
  },
);

it("handles a late rejected color save without feedback to a closed dialog", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  const gate = deferred<boolean>();
  vi.mocked(setTagColor).mockReturnValueOnce(gate.promise);
  color();
  await settle();
  await destroy();
  gate.reject(new Error("synthetic closed color failure"));
  await settle();
  expect(showToast).not.toHaveBeenCalled();
  expect(emit).not.toHaveBeenCalled();
});
