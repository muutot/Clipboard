import { createRawSnippet, flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Layout from "./+layout.svelte";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "#lib/services/settings.ts";
import { LIGHT_THEME_COLORS, DARK_THEME_COLORS } from "#lib/types/clipboard.ts";

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;
let media: EventTarget & { matches: boolean };

beforeEach(() => {
  media = Object.assign(new EventTarget(), { matches: false });
  vi.stubGlobal(
    "matchMedia",
    vi.fn(() => media),
  );
  generalSettings.merge({ ...DEFAULT_GENERAL_SETTINGS });
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
  generalSettings.destroy();
  vi.unstubAllGlobals();
});
function render() {
  component = mount(Layout, {
    target,
    props: { children: createRawSnippet(() => ({ render: () => "<span>Theme fixture</span>" })) },
  });
  flushSync();
}

it("updates document colors and code highlighting on OS changes and releases its listener", async () => {
  render();
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
  media.matches = true;
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(DARK_THEME_COLORS.bg);
  expect(document.documentElement.dataset.codeTheme).toBe("vs-dark");
  await unmount(component!);
  component = undefined;
  media.matches = false;
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(DARK_THEME_COLORS.bg);
});

it("keeps explicit and custom themes while the OS changes, then resumes system following", () => {
  render();
  generalSettings.merge({ theme: "custom", themeColors: { ...LIGHT_THEME_COLORS, bg: "#123456" } });
  media.matches = true;
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe("#123456");
  generalSettings.merge({ theme: "system" });
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(DARK_THEME_COLORS.bg);
  generalSettings.merge({ theme: "light", themeColors: LIGHT_THEME_COLORS });
  media.dispatchEvent(new Event("change"));
  expect(document.documentElement.style.getPropertyValue("--bg-app")).toBe(LIGHT_THEME_COLORS.bg);
});
