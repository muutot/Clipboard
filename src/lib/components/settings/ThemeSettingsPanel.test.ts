import { flushSync, mount, unmount } from "svelte";
import { get } from "svelte/store";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./ThemeSettingsPanel.svelte";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
import { locale } from "$lib/i18n";
import { LIGHT_THEME_COLORS, DARK_THEME_COLORS } from "$lib/types/clipboard";

let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
let media: EventTarget & { matches: boolean };
beforeEach(() => {
  media = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => media),
  );
  generalSettings.merge({ ...DEFAULT_GENERAL_SETTINGS });
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  flushSync();
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  generalSettings.destroy();
  vi.unstubAllGlobals();
});

it("renders system mode and readonly colors from the current system preference", () => {
  const selector = target.querySelector<HTMLButtonElement>('[aria-label="Theme Mode"]')!;
  expect(selector.textContent).toContain("Follow system");
  expect(target.querySelector<HTMLInputElement>('input[type="color"]')!.value).toBe(
    LIGHT_THEME_COLORS.bg,
  );
  media.matches = true;
  media.dispatchEvent(new Event("change"));
  flushSync();
  expect(target.querySelector<HTMLInputElement>('input[type="color"]')!.value).toBe(
    DARK_THEME_COLORS.bg,
  );
  expect(get(generalSettings).themeColors).toEqual(LIGHT_THEME_COLORS);
});

it("selects system as one coherent edit while clearing the active preset", () => {
  generalSettings.merge({
    theme: "custom",
    activePresetId: "fixture",
    themeColors: DARK_THEME_COLORS,
  });
  flushSync();
  target.querySelector<HTMLButtonElement>('[aria-label="Theme Mode"]')!.click();
  flushSync();
  const option = Array.from(target.querySelectorAll<HTMLButtonElement>("button")).find(
    (button) => button.textContent?.trim() === "Follow system",
  )!;
  option.click();
  flushSync();
  expect(get(generalSettings).theme).toBe("system");
  expect(get(generalSettings).activePresetId).toBeUndefined();
  expect(get(generalSettings).themeColors).toEqual(LIGHT_THEME_COLORS);
});

it("starts custom editing from the currently displayed system palette", () => {
  media.matches = true;
  media.dispatchEvent(new Event("change"));
  flushSync();
  target.querySelector<HTMLButtonElement>('[aria-label="Theme Mode"]')!.click();
  flushSync();
  Array.from(target.querySelectorAll<HTMLButtonElement>("button"))
    .find((button) => button.textContent?.trim() === "Custom")!
    .click();
  flushSync();
  expect(get(generalSettings).theme).toBe("custom");
  expect(get(generalSettings).themeColors).toEqual(DARK_THEME_COLORS);
});
