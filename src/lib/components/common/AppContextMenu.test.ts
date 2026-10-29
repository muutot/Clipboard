import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import AppContextMenu from "./AppContextMenu.svelte";
import { locale } from "$lib/i18n";
import { readClipboardText, writeClipboardText } from "$lib/services/clipboard";

vi.mock("$lib/services/clipboard", () => ({
  writeClipboardText: vi.fn(() => Promise.resolve()),
  readClipboardText: vi.fn(async () => "PASTE"),
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => (resolve = yes));
  return { promise, resolve };
}
function action(label: string) {
  const button = [...document.querySelectorAll<HTMLButtonElement>(".context-menu .menu-item")].find(
    (entry) => entry.textContent?.trim() === label,
  )!;
  button.click();
  flushSync();
}
async function settle() {
  for (let i = 0; i < 16; i++) {
    await Promise.resolve();
    flushSync();
  }
}

function rightClick(element: Element): MouseEvent {
  const event = new MouseEvent("contextmenu", {
    bubbles: true,
    cancelable: true,
    clientX: 24,
    clientY: 32,
  });
  element.dispatchEvent(event);
  flushSync();
  return event;
}

function menuLabels(): string[] {
  const menu = document.querySelector(".context-menu");
  if (!menu) return [];
  return [...menu.querySelectorAll(".menu-item .menu-label")].map(
    (label) => label.textContent?.trim() ?? "",
  );
}

beforeEach(() => {
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(AppContextMenu, { target });
  flushSync();
});

afterEach(async () => {
  window.getSelection()?.removeAllRanges();
  await unmount(component);
  target.remove();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

it("does not cut a later selection after the clipboard write completes", async () => {
  const field = document.createElement("textarea");
  field.value = "alpha beta";
  target.append(field);
  field.setSelectionRange(0, 5);
  const gate = deferred<void>();
  vi.mocked(writeClipboardText).mockReturnValueOnce(gate.promise);
  rightClick(field);
  action("Cut");
  expect(writeClipboardText).toHaveBeenCalledWith("alpha");
  expect(document.querySelector(".context-menu")).toBeNull();
  field.setSelectionRange(6, 10);
  gate.resolve();
  await settle();
  expect(field.value).toBe("alpha beta");
});

it("does not paste into a draft changed while reading the clipboard", async () => {
  const field = document.createElement("textarea");
  field.value = "initial draft";
  target.append(field);
  field.setSelectionRange(0, field.value.length);
  const gate = deferred<string>();
  vi.mocked(readClipboardText).mockReturnValueOnce(gate.promise);
  rightClick(field);
  action("Paste");
  field.value = "new draft";
  field.dispatchEvent(new Event("input", { bubbles: true }));
  gate.resolve("PASTE");
  await settle();
  expect(field.value).toBe("new draft");
});

it("does not execute contenteditable paste in a newly focused field", async () => {
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  editor.textContent = "original";
  const other = document.createElement("textarea");
  other.value = "other draft";
  target.append(editor, other);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  const range = document.createRange();
  range.selectNodeContents(editor);
  selection.addRange(range);
  const command = vi.fn(() => {
    if (document.activeElement === other) other.value += "PASTE";
    return true;
  });
  const previous = document.execCommand;
  document.execCommand = command;
  try {
    const gate = deferred<string>();
    vi.mocked(readClipboardText).mockReturnValueOnce(gate.promise);
    rightClick(editor);
    action("Paste");
    other.focus();
    gate.resolve("PASTE");
    await settle();
    expect(command).not.toHaveBeenCalled();
    expect(other.value).toBe("other draft");
  } finally {
    document.execCommand = previous;
  }
});

it.each(["Cut", "Paste"])(
  "finishes unchanged %s after the action closes its menu",
  async (label) => {
    const field = document.createElement("textarea");
    field.value = "alpha beta";
    target.append(field);
    const edited = vi.fn();
    field.addEventListener("input", edited);
    field.setSelectionRange(0, 5);
    const gate = deferred<void>();
    const read = deferred<string>();
    if (label === "Cut") vi.mocked(writeClipboardText).mockReturnValueOnce(gate.promise);
    else vi.mocked(readClipboardText).mockReturnValueOnce(read.promise);
    rightClick(field);
    action(label);
    expect(document.querySelector(".context-menu")).toBeNull();
    if (label === "Cut") gate.resolve();
    else read.resolve("PASTE");
    await settle();
    expect(field.value).toBe(label === "Cut" ? " beta" : "PASTE beta");
    expect(edited).toHaveBeenCalledOnce();
  },
);

it("cuts the original contenteditable range after the menu closes", async () => {
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  editor.textContent = "alpha beta";
  target.append(editor);
  const range = document.createRange();
  range.setStart(editor.firstChild!, 0);
  range.setEnd(editor.firstChild!, 5);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  const edited = vi.fn();
  editor.addEventListener("input", edited);
  const gate = deferred<void>();
  vi.mocked(writeClipboardText).mockReturnValueOnce(gate.promise);
  rightClick(editor);
  action("Cut");
  expect(document.querySelector(".context-menu")).toBeNull();
  gate.resolve();
  await settle();
  expect(editor.textContent).toBe(" beta");
  expect(edited).toHaveBeenCalledOnce();
});

it("restores a collapsed contenteditable caret before delayed paste", async () => {
  const editor = document.createElement("div");
  editor.setAttribute("contenteditable", "true");
  editor.tabIndex = 0;
  editor.textContent = "alpha beta";
  target.append(editor);
  const selection = window.getSelection()!;
  const range = document.createRange();
  range.setStart(editor.firstChild!, 6);
  range.collapse(true);
  selection.removeAllRanges();
  selection.addRange(range);
  const gate = deferred<string>();
  vi.mocked(readClipboardText).mockReturnValueOnce(gate.promise);
  const previous = document.execCommand;
  const command = vi.fn((name: string, _ui: boolean, text: string) => {
    if (name === "insertText") selection.getRangeAt(0).insertNode(document.createTextNode(text));
    return true;
  });
  document.execCommand = command;
  try {
    rightClick(editor);
    // The menu takes focus/selection before the edit action restores its target.
    const button = document.querySelector<HTMLButtonElement>(".context-menu button")!;
    button.focus();
    const menuRange = document.createRange();
    menuRange.selectNodeContents(button);
    selection.removeAllRanges();
    selection.addRange(menuRange);
    action("Paste");
    gate.resolve("PASTE");
    await settle();
    expect(command).toHaveBeenCalledWith("insertText", false, "PASTE");
    expect(editor.textContent).toBe("alpha PASTEbeta");
  } finally {
    document.execCommand = previous;
  }
});

it("suppresses the native menu without painting a menu of its own", () => {
  const plain = document.createElement("div");
  target.append(plain);

  const event = rightClick(plain);

  expect(event.defaultPrevented).toBe(true);
  expect(document.querySelector(".context-menu")).toBeNull();
});

it("opens the themed edit menu for text fields", () => {
  const input = document.createElement("input");
  target.append(input);

  rightClick(input);

  expect(menuLabels()).toEqual(["Cut", "Copy", "Paste", "Select All"]);
});

it("offers copy for a non-editable text selection", () => {
  const paragraph = document.createElement("p");
  paragraph.textContent = "Selected text";
  target.append(paragraph);
  const range = document.createRange();
  range.selectNodeContents(paragraph);
  const selection = window.getSelection()!;
  selection.removeAllRanges();
  selection.addRange(range);

  rightClick(paragraph);

  expect(menuLabels()).toEqual(["Copy"]);
  (document.querySelector(".context-menu .menu-item") as HTMLButtonElement).click();
  flushSync();
  expect(writeClipboardText).toHaveBeenCalledWith("Selected text");
});

it("ignores surfaces that already opened their own menu", () => {
  const handled = document.createElement("div");
  handled.addEventListener("contextmenu", (event) => event.preventDefault());
  target.append(handled);

  rightClick(handled);

  expect(document.querySelector(".context-menu")).toBeNull();
});
