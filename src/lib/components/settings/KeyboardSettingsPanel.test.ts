import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./KeyboardSettingsPanel.svelte";
import { locale } from "#lib/i18n/index.ts";

const bridge = vi.hoisted(() => ({ get: vi.fn(), configure: vi.fn(), reset: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("#lib/services/keyboard.ts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("#lib/services/keyboard.ts")>()),
  getKeyboardConfig: bridge.get,
  configureKeyboardShortcuts: bridge.configure,
  resetKeyboardConfig: bridge.reset,
}));

const conflict = {
  kind: "shortcut-conflict",
  shortcut: "Alt+5",
  firstAction: "switchFilter1",
  secondAction: "switchFilter5",
};
let target: HTMLDivElement;
let panel: ReturnType<typeof mount> | undefined;
async function settle() {
  for (let index = 0; index < 12; index++) {
    await Promise.resolve();
    flushSync();
  }
}
async function open(category: "switch" | "system" = "switch") {
  panel = mount(Panel, { target, props: { onclose() {}, showHeader: false, category } });
  await settle();
}
function allCard() {
  return [...target.querySelectorAll<HTMLElement>(".setting-card")].find((card) =>
    /切换到全部|Switch to All/.test(card.querySelector("strong")?.textContent ?? ""),
  )!;
}
async function record(key = "5", modifiers: KeyboardEventInit = { altKey: true }) {
  allCard().querySelector<HTMLButtonElement>(".binding-add")!.click();
  window.dispatchEvent(new KeyboardEvent("keydown", { key, ...modifiers }));
  await settle();
}
beforeEach(() => {
  locale.set("zh-CN");
  bridge.get.mockReset().mockResolvedValue({
    shortcuts: { switchFilter1: ["Alt+1"], switchFilter5: ["Alt+5"] },
  });
  bridge.configure.mockReset().mockRejectedValue(conflict);
  vi.spyOn(console, "warn").mockImplementation(() => {});
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (panel) await unmount(panel);
  panel = undefined;
  target.remove();
  vi.restoreAllMocks();
});

it.each([
  ["zh-CN", "快捷键 Alt+5 已被「切换到全部」和「切换到文件」同时使用"],
  ["en", 'Shortcut Alt+5 is assigned to both "Switch to All" and "Switch to File"'],
] as const)(
  "localizes shortcut conflict feedback in %s and retains saved bindings",
  async (lang, expected) => {
    locale.set(lang);
    await open();
    await record();
    expect(bridge.configure).toHaveBeenCalledWith("switchFilter1", ["Alt+1", "Alt+5"]);
    expect(target.querySelector(".settings-feedback")?.textContent).toBe(expected);
    expect(allCard().querySelector("kbd")?.textContent).toBe("Alt+1");
    expect(allCard().querySelectorAll("kbd")).toHaveLength(1);
  },
);

it("resolves numbered action labels and retains unknown custom identifiers", async () => {
  bridge.configure.mockRejectedValue({
    ...conflict,
    firstAction: "quickCopy3",
    secondAction: "customAction",
  });
  await open();
  await record();
  expect(target.querySelector(".settings-feedback")?.textContent).toBe(
    "快捷键 Alt+5 已被「快速复制列表第 3 条」和「customAction」同时使用",
  );
});

it.each([
  { kind: "failed", message: "disk unavailable" },
  "disk unavailable",
  {
    ...conflict,
    firstAction: null,
  },
])("shows localized save failure for other or malformed rejections", async (error) => {
  bridge.configure.mockRejectedValue(error);
  await open();
  await record();
  expect(target.querySelector(".settings-feedback")?.textContent).toBe("保存快捷键失败，请重试");
  expect(allCard().querySelector("kbd")?.textContent).toBe("Alt+1");
});

it("accepts a valid binding after a rejected conflict", async () => {
  bridge.configure.mockRejectedValueOnce(conflict).mockResolvedValueOnce(["Alt+1", "Ctrl+F12"]);
  await open();
  await record();
  await record("F12", { ctrlKey: true });
  expect(bridge.configure).toHaveBeenLastCalledWith("switchFilter1", ["Alt+1", "Ctrl+F12"]);
  expect([...allCard().querySelectorAll("kbd")].map((kbd) => kbd.textContent)).toEqual([
    "Alt+1",
    "Ctrl+F12",
  ]);
});

it("queues a keyboard reset behind an in-flight binding mutation", async () => {
  let finishConfigure!: (value: string[]) => void;
  bridge.configure.mockImplementation(
    () =>
      new Promise((resolve) => {
        finishConfigure = resolve;
      }),
  );
  bridge.reset.mockResolvedValue({ shortcuts: { switchFilter1: ["Alt+1"] } });
  await open("system");

  target.querySelector<HTMLButtonElement>(".binding-add")!.click();
  window.dispatchEvent(new KeyboardEvent("keydown", { key: "F12", ctrlKey: true }));
  await settle();
  expect(bridge.configure).toHaveBeenCalledTimes(1);

  target.querySelectorAll<HTMLButtonElement>(".config-bar-btn")[1].click();
  await settle();
  expect(bridge.reset).not.toHaveBeenCalled();

  finishConfigure(["Ctrl+F12"]);
  await settle();
  expect(bridge.reset).toHaveBeenCalledTimes(1);
});
