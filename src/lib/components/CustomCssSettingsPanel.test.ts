import { mount, unmount, flushSync } from "svelte";
import { get } from "svelte/store";
import { beforeEach, afterEach, expect, it, vi } from "vitest";
import Panel from "./CustomCssSettingsPanel.svelte";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
import { locale } from "$lib/i18n";

let target: HTMLDivElement;
let component: ReturnType<typeof mount> | undefined;
const saved = ":root { --accent: #123456; }";
async function settle() {
  for (let index = 0; index < 8; index++) {
    await Promise.resolve();
    flushSync();
  }
}
function button(label: string): HTMLButtonElement {
  return Array.from(target.querySelectorAll("button")).find(
    (item) => item.textContent?.trim() === label,
  )!;
}
beforeEach(() => {
  generalSettings.merge({ ...DEFAULT_GENERAL_SETTINGS, customCss: saved });
  locale.set("en");
  target = document.createElement("div");
  document.body.append(target);
});
afterEach(async () => {
  if (component) await unmount(component);
  component = undefined;
  target.remove();
  vi.restoreAllMocks();
});
it("waits for hydration and retains an unsaved draft across external updates", async () => {
  let ready!: () => void;
  vi.spyOn(generalSettings, "initialize").mockReturnValue(
    new Promise<void>((resolve) => {
      ready = resolve;
    }),
  );
  component = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  await settle();
  expect(target.querySelector("textarea")).toBeNull();
  ready();
  await settle();
  const editor = target.querySelector("textarea")!;
  expect(editor.value).toBe(saved);
  editor.value = ".clip-card { color: purple; }";
  editor.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  generalSettings.merge({ customCss: ":root { --accent: red; }", theme: "light" });
  await settle();
  expect(editor.value).toBe(".clip-card { color: purple; }");
  button("Discard edits").click();
  await settle();
  expect(editor.value).toBe(":root { --accent: red; }");
});
it("keeps code editable after persistence failure and permits an explicit retry", async () => {
  const flush = vi
    .spyOn(generalSettings, "flush")
    .mockRejectedValueOnce(new Error("disk unavailable"));
  component = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  await settle();
  button("Save and apply").click();
  await settle();
  expect(target.textContent).toContain("disk unavailable");
  expect(target.querySelector("textarea")!.value).toBe(saved);
  expect(button("Save and apply").disabled).toBe(false);
  button("Save and apply").click();
  await settle();
  expect(flush).toHaveBeenCalledTimes(2);
  expect(target.textContent).toContain("applied and saved");
});
it("refuses an entirely invalid stylesheet without replacing the saved value", async () => {
  component = mount(Panel, { target, props: { onclose() {}, showHeader: false } });
  await settle();
  const editor = target.querySelector("textarea")!;
  editor.value = "not a stylesheet";
  editor.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  button("Save and apply").click();
  await settle();
  expect(get(generalSettings).customCss).toBe(saved);
  expect(target.textContent).toContain("No valid CSS rules");
});
