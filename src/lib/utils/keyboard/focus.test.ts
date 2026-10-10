import { afterEach, describe, expect, it } from "vitest";

import { getFocusableElements, trapTabFocus } from "./focus";

function buildDialog(): {
  container: HTMLElement;
  first: HTMLElement;
  middle: HTMLElement;
  last: HTMLElement;
  outside: HTMLElement;
} {
  const outside = document.createElement("button");
  const container = document.createElement("div");
  const first = document.createElement("button");
  const input = document.createElement("input");
  const disabled = document.createElement("button");
  disabled.disabled = true;
  const hidden = document.createElement("button");
  hidden.style.display = "none";
  const last = document.createElement("textarea");
  container.append(first, input, disabled, hidden, last);
  document.body.append(outside, container);
  return { container, first, middle: input, last, outside };
}

describe("getFocusableElements", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("skips disabled and display:none elements", () => {
    const { container, first, middle, last } = buildDialog();
    const focusable = getFocusableElements(container);
    expect(focusable).toEqual([first, middle, last]);
  });
});

describe("trapTabFocus", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  function tabEvent(shift: boolean): KeyboardEvent {
    return new KeyboardEvent("keydown", { key: "Tab", shiftKey: shift, cancelable: true });
  }

  it("wraps forward from the last element to the first", () => {
    const { container, first, last } = buildDialog();
    last.focus();
    trapTabFocus(container, tabEvent(false));
    expect(document.activeElement).toBe(first);
  });

  it("wraps backward with shift from the first element", () => {
    const { container, first, last } = buildDialog();
    first.focus();
    trapTabFocus(container, tabEvent(true));
    expect(document.activeElement).toBe(last);
  });

  it("pulls focus back inside when focus escaped the dialog", () => {
    const { container, first, outside } = buildDialog();
    outside.focus();
    trapTabFocus(container, tabEvent(false));
    expect(document.activeElement).toBe(first);
  });

  it("leaves non-Tab events untouched", () => {
    const { container } = buildDialog();
    const event = new KeyboardEvent("keydown", { key: "Enter", cancelable: true });
    trapTabFocus(container, event);
    expect(event.defaultPrevented).toBe(false);
  });
});
