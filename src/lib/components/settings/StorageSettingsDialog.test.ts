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

// The lazy panel chunk resolves on real macrotasks, so wait on the clock instead
// of a fixed tick count: the old 40-tick budget is barely 0.2 s and expires
// before the chunk arrives on a loaded CI runner.
async function waitFor<T>(find: () => T | null, timeoutMs = 2000): Promise<T> {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    const found = find();
    if (found) return found;
    if (Date.now() >= deadline) throw new Error("element did not appear");
    await settle(1);
  }
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

    const input = await waitFor(() =>
      target.querySelector<HTMLInputElement>("#settings-search-input"),
    );
    input.value = t("general.fontSizeCardTitleLabel");
    input.dispatchEvent(new Event("input", { bubbles: true }));

    const result = await waitFor(() =>
      target.querySelector<HTMLButtonElement>(
        '.settings-search-results [data-settings-search-id="font.card-title"]',
      ),
    );
    result.click();

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
    input.value = t("general.fontSizeBaseLabel");
    input.dispatchEvent(new Event("input", { bubbles: true }));
    const baseResult = await waitFor(() =>
      target.querySelector<HTMLButtonElement>(
        '.settings-search-results [data-settings-search-id="font.base"]',
      ),
    );
    baseResult.click();
    const interfaceTab = await waitFor(() => {
      const button = target.querySelector<HTMLButtonElement>(".font-subnav button.active");
      return button?.textContent?.trim() === t("general.fontSizeInterfaceTab") ? button : null;
    });
    expect(interfaceTab.textContent?.trim()).toBe(t("general.fontSizeInterfaceTab"));
    expect(target.querySelector('[data-settings-search-id="font.base"]')).not.toBeNull();
  });
});
