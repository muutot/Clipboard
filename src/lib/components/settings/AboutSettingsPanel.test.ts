import { flushSync, mount, unmount } from "svelte";
import { afterAll, afterEach, beforeAll, expect, it, vi } from "vitest";
import { t } from "$lib/i18n";
import { invokeTauriRequired } from "$lib/services/runtime";
import type { UpdateInfo } from "$lib/services/update";
import AboutSettingsPanel from "./AboutSettingsPanel.svelte";

vi.mock("$lib/services/runtime", () => ({
  isTauriRuntime: () => true,
  invokeTauriRequired: vi.fn(),
}));
vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return {
    generalSettings: Object.assign(
      writable({ updateSource: "github", devtoolsShortcutEnabled: false }),
      { updateSetting: vi.fn() },
    ),
  };
});

const latest: UpdateInfo = {
  currentVersion: "1.8.2",
  latestVersion: "2.0.0",
  updateAvailable: true,
  releaseUrl: "https://example.invalid/v2",
  releaseTitle: "Latest",
  releaseNotes: "NEW_NOTES",
  publishedAt: null,
};
const current: UpdateInfo = {
  ...latest,
  latestVersion: "1.8.2",
  updateAvailable: false,
  releaseUrl: "https://example.invalid/v1",
  releaseTitle: "Current",
  releaseNotes: "CURRENT_NOTES",
};
const cleanups: (() => Promise<void>)[] = [];
const originalModal = Object.getOwnPropertyDescriptor(HTMLDialogElement.prototype, "showModal");
beforeAll(() => {
  Object.defineProperty(HTMLDialogElement.prototype, "showModal", {
    configurable: true,
    value(this: HTMLDialogElement) {
      this.open = true;
    },
  });
});
afterAll(() => {
  if (originalModal) Object.defineProperty(HTMLDialogElement.prototype, "showModal", originalModal);
  else Reflect.deleteProperty(HTMLDialogElement.prototype, "showModal");
});
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.resetAllMocks();
});
async function settle() {
  for (let i = 0; i < 16; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function deferred() {
  let resolve!: (value: UpdateInfo) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<UpdateInfo>((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function render() {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(AboutSettingsPanel, {
    target,
    props: { appVersion: "1.8.2", appExecutablePath: "/fixture/Clipboard", onclose: () => {} },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}
function click(target: HTMLElement, key: string) {
  [...target.querySelectorAll<HTMLButtonElement>(".about-update-controls > button")]
    .find((button) => button.textContent?.trim() === t(key))!
    .click();
  flushSync();
}

it("does not claim up to date when only current release notes were fetched", async () => {
  vi.mocked(invokeTauriRequired).mockResolvedValueOnce(current);
  const target = render();
  click(target, "about.releaseNotes");
  await settle();
  expect(invokeTauriRequired).toHaveBeenCalledWith(
    "get_release",
    { version: "1.8.2" },
    expect.any(String),
  );
  expect(target.querySelector(".update-dialog")?.textContent).toContain("CURRENT_NOTES");
  expect(target.querySelector('[role="status"]')).toBeNull();
});

it("keeps the newer update dialog when older current release notes arrive", async () => {
  const notes = deferred();
  const check = deferred();
  vi.mocked(invokeTauriRequired).mockImplementation((command) =>
    command === "get_release" ? notes.promise : check.promise,
  );
  const target = render();
  click(target, "about.releaseNotes");
  click(target, "about.checkUpdate");
  check.resolve(latest);
  await settle();
  notes.resolve(current);
  await settle();
  expect(target.querySelector(".update-dialog")?.textContent).toContain("NEW_NOTES");
  expect(target.querySelector(".update-dialog a")?.getAttribute("href")).toBe(latest.releaseUrl);
  expect(target.querySelector('[role="status"]')).toBeNull();
});

it("keeps requested current notes when an older update check arrives", async () => {
  const notes = deferred();
  const check = deferred();
  vi.mocked(invokeTauriRequired).mockImplementation((command) =>
    command === "get_release" ? notes.promise : check.promise,
  );
  const target = render();
  click(target, "about.checkUpdate");
  click(target, "about.releaseNotes");
  notes.resolve(current);
  await settle();
  check.resolve(latest);
  await settle();
  expect(target.querySelector(".update-dialog")?.textContent).toContain("CURRENT_NOTES");
  expect(target.querySelector(".update-dialog a")).toBeNull();
});

it("shows a current-notes failure even after a successful latest check", async () => {
  vi.mocked(invokeTauriRequired).mockResolvedValueOnce(current);
  const target = render();
  click(target, "about.checkUpdate");
  await settle();
  expect(target.querySelector('[role="status"]')?.textContent).toContain(t("about.upToDate"));
  vi.mocked(invokeTauriRequired).mockRejectedValueOnce(new Error("notes unavailable"));
  click(target, "about.releaseNotes");
  await settle();
  expect(target.querySelector('[role="alert"]')?.textContent).toContain("notes unavailable");
});

it("shows a normal update result and releases the pending check guard", async () => {
  vi.mocked(invokeTauriRequired).mockResolvedValue(latest);
  const target = render();
  click(target, "about.checkUpdate");
  await settle();
  expect(target.querySelector(".update-dialog")?.textContent).toContain("NEW_NOTES");
  expect(target.querySelector(".update-dialog a")?.getAttribute("href")).toBe(latest.releaseUrl);
  expect(
    [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent?.trim() === t("about.checkUpdate"),
    )?.disabled,
  ).toBe(false);
});

it("does not show an older check dialog while newer current notes are pending", async () => {
  const notes = deferred();
  const check = deferred();
  vi.mocked(invokeTauriRequired).mockImplementation((command) =>
    command === "get_release" ? notes.promise : check.promise,
  );
  const target = render();
  click(target, "about.checkUpdate");
  click(target, "about.releaseNotes");
  check.resolve(latest);
  await settle();
  expect(target.querySelector(".update-dialog")).toBeNull();
  notes.resolve(current);
  await settle();
  expect(target.querySelector(".update-dialog")?.textContent).toContain("CURRENT_NOTES");
});

it("does not reopen a closed notes dialog for an older update response", async () => {
  const check = deferred();
  vi.mocked(invokeTauriRequired).mockImplementation((command) =>
    command === "get_release" ? Promise.resolve(current) : check.promise,
  );
  const target = render();
  click(target, "about.checkUpdate");
  click(target, "about.releaseNotes");
  await settle();
  target.querySelector<HTMLButtonElement>(`button[aria-label="${t("about.close")}"]`)!.click();
  flushSync();
  check.resolve(latest);
  await settle();
  expect(target.querySelector(".update-dialog")).toBeNull();
});

it("clears current-notes failure on a successful retry", async () => {
  vi.mocked(invokeTauriRequired).mockRejectedValueOnce(new Error("retry notes"));
  const target = render();
  click(target, "about.releaseNotes");
  await settle();
  expect(target.querySelector('[role="alert"]')?.textContent).toContain(
    t("about.releaseNotesFailed", { error: "retry notes" }),
  );
  vi.mocked(invokeTauriRequired).mockResolvedValueOnce(current);
  click(target, "about.releaseNotes");
  await settle();
  expect(target.querySelector('[role="alert"]')).toBeNull();
  expect(target.querySelector('[role="status"]')).toBeNull();
  expect(target.querySelector(".update-dialog")?.textContent).toContain("CURRENT_NOTES");
});
