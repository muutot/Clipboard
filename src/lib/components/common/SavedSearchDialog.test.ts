import { flushSync, mount, unmount } from "svelte";
import { get } from "svelte/store";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import SavedSearchDialog from "./SavedSearchDialog.svelte";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
import type { SavedSearchState } from "$lib/utils/settings/saved-searches";

const state: SavedSearchState = {
  query: "账",
  activeFilter: "favorite",
  tagFilter: "work",
  sourceAppFilter: "Editor",
  dateFilter: "week",
  sortRules: [{ field: "size", direction: "desc" }],
};
let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const apply = vi.fn();
const close = vi.fn();
async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}
const button = (text: string) =>
  [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;
function input(value: string) {
  const element = target.querySelector("input")!;
  element.value = value;
  element.dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}
beforeEach(() => {
  generalSettings.set({ ...DEFAULT_GENERAL_SETTINGS, language: "en", savedSearches: [] });
  HTMLDialogElement.prototype.showModal = function () {
    this.setAttribute("open", "");
  };
  HTMLDialogElement.prototype.close = function () {
    this.removeAttribute("open");
  };
  target = document.createElement("div");
  document.body.append(target);
  component = mount(SavedSearchDialog, {
    target,
    props: { current: state, onapply: apply, onclose: close },
  });
  flushSync();
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  vi.restoreAllMocks();
  vi.clearAllMocks();
});
it("saves complete state, applies, renames and deletes", async () => {
  input("Work");
  button("Save current").click();
  await settle();
  expect(get(generalSettings).savedSearches[0]).toMatchObject({ ...state, name: "Work" });
  button("Work").click();
  expect(apply).toHaveBeenCalledWith(expect.objectContaining(state));
  button("Rename").click();
  flushSync();
  const rename = target.querySelectorAll("input")[1];
  rename.value = "Renamed";
  rename.dispatchEvent(new Event("input", { bubbles: true }));
  button("Rename").click();
  await settle();
  expect(button("Renamed")).toBeTruthy();
  button("Delete").click();
  await settle();
  expect(get(generalSettings).savedSearches).toEqual([]);
});
it("keeps pending changes visible and offers retry when persistence fails", async () => {
  vi.spyOn(generalSettings, "flush")
    .mockRejectedValueOnce(new Error("disk full"))
    .mockResolvedValueOnce();
  input("Work");
  button("Save current").click();
  await settle();
  expect(target.querySelector('[role="alert"]')).not.toBeNull();
  expect(button("Work")).toBeTruthy();
  button("Retry").click();
  await settle();
  expect(target.querySelector('[role="alert"]')).toBeNull();
});
