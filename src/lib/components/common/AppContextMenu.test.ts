import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import AppContextMenu from "./AppContextMenu.svelte";
import { locale } from "$lib/i18n";
import { writeClipboardText } from "$lib/services/clipboard";

vi.mock("$lib/services/clipboard", () => ({
  writeClipboardText: vi.fn(() => Promise.resolve()),
}));

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;

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
