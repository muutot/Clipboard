import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import Toolbar from "./Toolbar.svelte";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.restoreAllMocks();
});

function mountToolbar(): HTMLElement {
  generalSettings.set({ ...DEFAULT_GENERAL_SETTINGS, language: "en" });
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(Toolbar, {
    target,
    props: {
      filters: [{ id: "all", label: "All", icon: "text" }],
      activeFilter: "all",
      filterShortcutBindings: {},
      onselectfilter: () => {},
      sourceApps: ["Browser"],
      sourceAppFilter: "",
      onsourceapp: () => {},
      dateFilter: "all",
      dateFilterOptions: [{ id: "today", label: "Today" }],
      ondatefilter: () => {},
      onsettings: () => {},
    },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}

function pressEscape(): void {
  document.body.dispatchEvent(
    new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
  );
  flushSync();
}

describe("Toolbar dropdowns", () => {
  it("Escape closes the open source-app dropdown and refocuses the toggle", () => {
    const target = mountToolbar();
    const toggle = target.querySelector<HTMLButtonElement>(".filter-dropdown-btn")!;
    expect(toggle.getAttribute("aria-expanded")).toBe("false");

    toggle.click();
    flushSync();
    expect(toggle.getAttribute("aria-expanded")).toBe("true");

    pressEscape();
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(toggle);
  });

  it("Escape closes the open date dropdown and refocuses the toggle", () => {
    const target = mountToolbar();
    const toggles = target.querySelectorAll<HTMLButtonElement>(".filter-dropdown-btn");
    const dateToggle = toggles[1]!;
    expect(dateToggle.getAttribute("aria-expanded")).toBe("false");

    dateToggle.click();
    flushSync();
    expect(dateToggle.getAttribute("aria-expanded")).toBe("true");

    pressEscape();
    expect(dateToggle.getAttribute("aria-expanded")).toBe("false");
    expect(document.activeElement).toBe(dateToggle);
  });

  it("Escape with the dropdown closed is ignored", () => {
    const target = mountToolbar();
    const toggle = target.querySelector<HTMLButtonElement>(".filter-dropdown-btn")!;
    pressEscape();
    expect(toggle.getAttribute("aria-expanded")).toBe("false");
  });
});
