import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, it, vi } from "vitest";
import { t } from "$lib/i18n";
import { writeClipboardText } from "$lib/services/clipboard";
import CodeEditor from "./CodeEditor.svelte";

vi.mock("$lib/services/clipboard", () => ({
  writeClipboardText: vi.fn(async () => {}),
  readClipboardText: vi.fn(async () => ""),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  window.getSelection()?.removeAllRanges();
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.clearAllMocks();
  vi.restoreAllMocks();
});

async function settle() {
  for (let i = 0; i < 16; i++) {
    await Promise.resolve();
    flushSync();
  }
}

async function render(body: string) {
  const store = writable(body);
  const state = fromStore(store);
  const change = vi.fn((value: string) => store.set(value));
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(CodeEditor, {
    target,
    props: {
      get content() {
        return state.current;
      },
      language: "text",
      editorLabel: "Editor",
      oncontentchange: change,
    },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  await settle();
  const editor = target.querySelector<HTMLElement>('[role="textbox"]')!;
  const range = document.createRange();
  range.selectNodeContents(editor);
  window.getSelection()!.removeAllRanges();
  window.getSelection()!.addRange(range);
  return { target, editor, range, change };
}

it.each(["alpha\u200b\n", "alpha\u200b"])(
  "context copy preserves %j without presentation text",
  async (body) => {
    const { target, editor } = await render(body);
    editor.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    flushSync();
    [...target.querySelectorAll<HTMLButtonElement>(".context-menu button")]
      .find((button) => button.textContent?.trim() === t("edit.copy"))!
      .click();
    await settle();
    expect(writeClipboardText).toHaveBeenCalledWith(body);
  },
);

it("context cut writes clean text and removes the selected draft", async () => {
  const body = "alpha\u200b\n";
  const { target, editor, change } = await render(body);
  editor.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
  flushSync();
  [...target.querySelectorAll<HTMLButtonElement>(".context-menu button")]
    .find((button) => button.textContent?.trim() === t("edit.cut"))!
    .click();
  await settle();
  expect(writeClipboardText).toHaveBeenCalledWith(body);
  expect(change).toHaveBeenLastCalledWith("");
});

it.each(["copy", "cut"])(
  "intercepts native %s when selection contains presentation text",
  async (action) => {
    const body = "alpha\u200b\n";
    const { editor } = await render(body);
    const setData = vi.fn();
    const event = new Event(action, { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData } });
    editor.dispatchEvent(event);
    flushSync();
    expect(event.defaultPrevented).toBe(true);
    expect(setData).toHaveBeenCalledWith("text/plain", body);
    if (action === "cut") expect(editor.textContent).toBe("");
  },
);

it("keeps the draft when context cut cannot write the clipboard", async () => {
  vi.spyOn(console, "error").mockImplementation(() => {});
  vi.mocked(writeClipboardText).mockRejectedValueOnce(new Error("clipboard unavailable"));
  const { target, editor, change } = await render("alpha\u200b\n");
  editor.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
  flushSync();
  [...target.querySelectorAll<HTMLButtonElement>(".context-menu button")]
    .find((button) => button.textContent?.trim() === t("edit.cut"))!
    .click();
  await settle();
  expect(editor.textContent).toBe("alpha\u200b\n\u200b");
  expect(change).not.toHaveBeenCalled();
});

it("uses the native delete command for intercepted cut when supported", async () => {
  const { editor, range, change } = await render("alpha\u200b\n");
  const original = document.execCommand;
  const command = vi.fn(() => {
    range.deleteContents();
    editor.dispatchEvent(new Event("input", { bubbles: true }));
    return true;
  });
  document.execCommand = command;
  try {
    const event = new Event("cut", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "clipboardData", { value: { setData: vi.fn() } });
    editor.dispatchEvent(event);
    flushSync();
    expect(command).toHaveBeenCalledWith("delete");
    expect(change).toHaveBeenLastCalledWith("");
  } finally {
    document.execCommand = original;
  }
});

it("leaves native copy unchanged when selection excludes the marker", async () => {
  const { editor, range } = await render("alpha\u200b\n");
  range.setEnd(editor.firstChild!, 6);
  const setData = vi.fn();
  const event = new Event("copy", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { setData } });
  editor.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(false);
  expect(setData).not.toHaveBeenCalled();
  expect(range.toString()).toBe("alpha\u200b");
});

it("does not write a display-only selection to the clipboard", async () => {
  const { editor, range } = await render("alpha\u200b\n");
  range.selectNodeContents(editor.lastChild!);
  const setData = vi.fn();
  const event = new Event("copy", { bubbles: true, cancelable: true });
  Object.defineProperty(event, "clipboardData", { value: { setData } });
  editor.dispatchEvent(event);
  expect(event.defaultPrevented).toBe(true);
  expect(setData).not.toHaveBeenCalled();
});
