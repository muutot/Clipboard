import { mount, unmount, flushSync } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./TagRulesSettingsPanel.svelte";
import { locale } from "$lib/i18n";
import * as service from "$lib/services/clipboard";

vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/clipboard", () => ({
  getAutoTagRules: vi.fn(),
  setAutoTagRules: vi.fn(),
  previewAutoTagRules: vi.fn(),
  previewAutoTagHistory: vi.fn(),
  applyAutoTagHistory: vi.fn(),
}));
let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const rule = { pattern: "TODO", tag: "work", sourceApp: "Editor", kind: "text" as const };
const result = {
  matchedCount: 2,
  changedCount: 1,
  samples: [{ id: "1", title: "TODO", tags: ["work"] }],
};
async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}
const button = (text: string) =>
  [...target.querySelectorAll("button")].find((b) => b.textContent?.trim() === text)!;
beforeEach(async () => {
  locale.set("en");
  vi.mocked(service.getAutoTagRules).mockResolvedValue([{ ...rule }]);
  vi.mocked(service.setAutoTagRules).mockResolvedValue([rule]);
  vi.mocked(service.previewAutoTagHistory).mockResolvedValue(result);
  vi.mocked(service.applyAutoTagHistory).mockResolvedValue(result);
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, { target, props: { onclose: () => {} } });
  await settle();
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  vi.clearAllMocks();
});
it("saves source and type, previews before applying, then resets the preview", async () => {
  button("Save rules").click();
  await settle();
  expect(service.setAutoTagRules).toHaveBeenCalledWith([rule]);
  expect(button("Apply to history").disabled).toBe(true);
  button("Preview history").click();
  await settle();
  expect(service.previewAutoTagHistory).toHaveBeenCalledWith([rule]);
  expect(button("Apply to history").disabled).toBe(false);
  button("Apply to history").click();
  await settle();
  expect(service.applyAutoTagHistory).toHaveBeenCalledWith([rule]);
  expect(button("Apply to history").disabled).toBe(true);
});
it("invalidates historical previews on edits and displays failures", async () => {
  button("Preview history").click();
  await settle();
  const input = target.querySelector<HTMLInputElement>(".autotag-pattern")!;
  input.value = "FIXME";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  expect(button("Apply to history").disabled).toBe(true);
  vi.mocked(service.previewAutoTagHistory).mockRejectedValueOnce(new Error("disk unavailable"));
  button("Preview history").click();
  await settle();
  expect(target.textContent).toContain("disk unavailable");
  expect(button("Preview history").disabled).toBe(false);
});
