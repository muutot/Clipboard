import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { setLocale, t } from "$lib/i18n";
import OnboardingOverlay from "./OnboardingOverlay.svelte";

// The overlay is the first-run teaching surface: it must walk its steps in
// order, guard the first step, and report completion exactly once through
// `onclose` — the route persists the flag from that callback.

const mounted: (() => void)[] = [];
afterEach(() => {
  while (mounted.length) mounted.pop()?.();
});

function render() {
  const onclose = vi.fn();
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(OnboardingOverlay, { target, props: { onclose } });
  mounted.push(() => {
    unmount(app);
    target.remove();
  });
  flushSync();
  const button = (label: string) =>
    [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.textContent?.trim() === label,
    )!;
  return { target, onclose, button };
}

describe("OnboardingOverlay", () => {
  beforeEach(() => {
    setLocale("zh-CN");
  });

  it("opens on the first step with Back disabled", () => {
    const { target, button } = render();

    const title = target.querySelector<HTMLElement>("#onboarding-title")!;
    expect(title.textContent).toBe(t("onboarding.steps.welcome.title"));
    expect(target.querySelector(".onboarding-progress")!.textContent).toBe(
      t("onboarding.progress", { current: 1, total: 6 }),
    );
    expect(button(t("onboarding.prev")).disabled).toBe(true);
    expect(button(t("onboarding.finish"))).toBeUndefined();
  });

  it("walks forward and backward without closing early", () => {
    const { target, button, onclose } = render();

    button(t("onboarding.next")).click();
    flushSync();
    expect(target.querySelector("#onboarding-title")!.textContent).toBe(
      t("onboarding.steps.search.title"),
    );
    expect(onclose).not.toHaveBeenCalled();

    button(t("onboarding.prev")).click();
    flushSync();
    expect(target.querySelector("#onboarding-title")!.textContent).toBe(
      t("onboarding.steps.welcome.title"),
    );
    expect(onclose).not.toHaveBeenCalled();
  });

  it("closes once on the last step's finish action", () => {
    const { button, onclose } = render();

    for (let step = 0; step < 5; step += 1) {
      button(t("onboarding.next")).click();
      flushSync();
    }
    expect(button(t("onboarding.finish"))).toBeDefined();

    button(t("onboarding.finish")).click();
    flushSync();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it("closes from the skip action", () => {
    const { button, onclose } = render();

    button(t("onboarding.skip")).click();
    flushSync();
    expect(onclose).toHaveBeenCalledTimes(1);
  });

  it("closes on Escape and swallows the key so the route cannot hide the window", () => {
    const { target, onclose } = render();
    const panel = target.querySelector<HTMLElement>(".onboarding-panel")!;

    const event = new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true });
    panel.dispatchEvent(event);
    flushSync();

    expect(onclose).toHaveBeenCalledTimes(1);
    expect(event.defaultPrevented).toBe(true);
  });
});
