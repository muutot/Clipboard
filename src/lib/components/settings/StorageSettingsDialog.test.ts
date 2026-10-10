import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import StorageSettingsDialog from "./StorageSettingsDialog.svelte";
import { t } from "$lib/i18n";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.unstubAllGlobals();
});

async function settle(rounds = 24) {
  for (let i = 0; i < rounds; i++) {
    await Promise.resolve();
    flushSync();
    // Lazy panel chunks resolve on a macrotask in the test environment.
    await new Promise((resolve) => setTimeout(resolve, 0));
  }
}

async function waitFor<T>(find: () => T | null, rounds = 40): Promise<T> {
  for (let i = 0; i < rounds; i++) {
    const found = find();
    if (found) return found;
    await settle(1);
  }
  throw new Error("element did not appear");
}

describe("StorageSettingsDialog settings search", () => {
  it("opens the card font sub-tab for a title-font search result", async () => {
    vi.stubGlobal("scrollIntoView", () => {});
    Element.prototype.scrollIntoView = () => {};
    const target = document.createElement("div");
    document.body.append(target);
    const app = mount(StorageSettingsDialog, {
      target,
      props: { open: true, onclose: () => {} },
    });
    cleanups.push(async () => {
      await unmount(app);
      target.remove();
    });
    await settle();

    const input = target.querySelector<HTMLInputElement>("#settings-search-input");
    expect(input).not.toBeNull();
    input!.value = t("general.fontSizeCardTitleLabel");
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();

    const result = target.querySelector<HTMLButtonElement>(
      '.settings-search-results [data-settings-search-id="font.card-title"]',
    );
    expect(result).not.toBeNull();
    result!.click();

    // The card sub-tab is active and its slider (which carries the search id)
    // is the one now rendered, not the interface sliders.
    const activeTab = await waitFor(() =>
      target.querySelector<HTMLButtonElement>(".font-subnav button.active"),
    );
    expect(activeTab.textContent?.trim()).toBe(t("general.fontSizeCardTab"));
    expect(target.querySelector('[data-settings-search-id="font.card-title"]')).not.toBeNull();
    expect(target.querySelector('[data-settings-search-id="font.base"]')).toBeNull();

    // A second jump while already on the font section must also move the
    // sub-tab, which exercises the dialog-owned prop update.
    input!.value = t("general.fontSizeBaseLabel");
    input!.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    target
      .querySelector<HTMLButtonElement>(
        '.settings-search-results [data-settings-search-id="font.base"]',
      )!
      .click();
    const interfaceTab = await waitFor(() => {
      const button = target.querySelector<HTMLButtonElement>(".font-subnav button.active");
      return button?.textContent?.trim() === t("general.fontSizeInterfaceTab") ? button : null;
    });
    expect(interfaceTab.textContent?.trim()).toBe(t("general.fontSizeInterfaceTab"));
    expect(target.querySelector('[data-settings-search-id="font.base"]')).not.toBeNull();
  });
});
