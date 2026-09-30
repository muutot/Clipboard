import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import CustomSelect, { type CustomSelectOption } from "./CustomSelect.svelte";

// `CustomSelect` is the only settings dropdown, and the trigger carries the
// `settings-select` class whose styling contract changed when the dead
// data-URI arrow was removed. Locking its a11y and interaction surface keeps
// that styling change honest: the arrow is an `AppIcon` inside the button, the
// value is a `.custom-select-value` span, and the popover is a real listbox.

const OPTIONS: CustomSelectOption[] = [
  { value: "en", label: "English" },
  { value: "zh-CN", label: "简体中文" },
  { value: "off", label: "Disabled one", disabled: true },
];

const mounted: (() => void)[] = [];
afterEach(() => {
  while (mounted.length) mounted.pop()?.();
});

function render(props: Partial<{ value: string | number; disabled: boolean }> = {}) {
  const onchange = vi.fn();
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(CustomSelect, {
    target,
    props: {
      value: "en",
      options: OPTIONS,
      onchange,
      ariaLabel: "Language",
      ...props,
    },
  });
  mounted.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  const trigger = () => target.querySelector<HTMLButtonElement>("button.settings-select")!;
  const popover = () => target.querySelector<HTMLElement>('[role="listbox"]');
  const optionButtons = () => [...target.querySelectorAll<HTMLButtonElement>('[role="option"]')];
  return { target, onchange, trigger, popover, optionButtons };
}

describe("CustomSelect", () => {
  it("shows the selected label and the icon arrow, not an inline background", () => {
    const { trigger } = render();
    const value = trigger().querySelector(".custom-select-value");
    expect(value?.textContent?.trim()).toBe("English");
    // The arrow is an icon element so it can inherit the themed text color; a
    // data-URI background on the trigger would defeat theming and is gone.
    expect(trigger().querySelector("svg")).not.toBeNull();
    expect(getComputedStyle(trigger()).backgroundImage).toBe("none");
  });

  it("falls back to the raw value when no option matches", () => {
    expect(render({ value: "fr" }).trigger().textContent).toContain("fr");
  });

  it("exposes listbox semantics and toggles the popover", () => {
    const { trigger, popover, optionButtons } = render();
    expect(trigger().getAttribute("aria-haspopup")).toBe("listbox");
    expect(trigger().getAttribute("aria-expanded")).toBe("false");
    expect(popover()).toBeNull();

    trigger().click();
    flushSync();
    expect(trigger().getAttribute("aria-expanded")).toBe("true");
    expect(popover()?.getAttribute("aria-label")).toBe("Language");
    expect(optionButtons().map((b) => b.getAttribute("aria-selected"))).toEqual([
      "true",
      "false",
      "false",
    ]);
    expect(optionButtons()[2].disabled).toBe(true);

    trigger().click();
    flushSync();
    expect(popover()).toBeNull();
  });

  it("reports the picked value and closes, ignoring disabled options", () => {
    const { trigger, onchange, optionButtons, popover } = render();
    trigger().click();
    flushSync();

    // Re-picking the current value dismisses the popover without reporting a
    // change — clicking the highlighted row is a dismissal, not a selection.
    optionButtons()[0].click();
    flushSync();
    expect(onchange).not.toHaveBeenCalled();
    expect(popover()).toBeNull();

    trigger().click();
    flushSync();
    optionButtons()[1].click();
    flushSync();
    expect(onchange).toHaveBeenCalledExactlyOnceWith("zh-CN");
    expect(popover()).toBeNull();

    trigger().click();
    flushSync();
    optionButtons()[2].click();
    flushSync();
    expect(onchange).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and on an outside scroll, but not on its own scroll", () => {
    const { trigger, popover } = render();
    trigger().click();
    flushSync();

    // Svelte 5 delegates keydown to the mount root, so the event has to bubble.
    trigger().dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true }));
    flushSync();
    expect(popover()).toBeNull();

    trigger().click();
    flushSync();
    // A scroll inside the popover must not close it mid-read.
    popover()!.dispatchEvent(new Event("scroll", { bubbles: true }));
    flushSync();
    expect(popover()).not.toBeNull();

    window.dispatchEvent(new Event("scroll"));
    flushSync();
    expect(popover()).toBeNull();
  });

  it("never opens while disabled", () => {
    const { trigger, popover } = render({ disabled: true });
    trigger().click();
    flushSync();
    expect(popover()).toBeNull();
    expect(trigger().disabled).toBe(true);
  });
});
