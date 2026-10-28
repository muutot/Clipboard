import { mount, unmount, flushSync } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./SourceMappingSettingsPanel.svelte";
import { locale } from "$lib/i18n";
import * as service from "$lib/services/clipboard";

vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("$lib/services/clipboard", () => ({
  getSourceMappingRules: vi.fn(),
  setSourceMappingRules: vi.fn(),
}));
let target: HTMLDivElement;
let component: ReturnType<typeof mount>;
const feedback = vi.fn();
const rule = { domain: "github.com", titleKeyword: "Pull Request", name: "GitHub PRs" };
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
  vi.mocked(service.getSourceMappingRules).mockResolvedValue([{ ...rule }]);
  vi.mocked(service.setSourceMappingRules).mockResolvedValue([rule]);
  target = document.createElement("div");
  document.body.append(target);
  component = mount(Panel, {
    target,
    props: { onclose: () => {}, onfeedback: feedback },
  });
  await settle();
});
afterEach(async () => {
  await unmount(component);
  target.remove();
  vi.clearAllMocks();
});
it("hydrates saved rules and saves trimmed snapshots with success feedback", async () => {
  expect(target.querySelector<HTMLInputElement>(".mapping-domain")!.value).toBe("github.com");
  expect(target.querySelector<HTMLInputElement>(".mapping-name")!.value).toBe("GitHub PRs");
  const input = target.querySelector<HTMLInputElement>(".mapping-domain")!;
  input.value = "  example.com  ";
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  button("Save rules").click();
  await settle();
  expect(service.setSourceMappingRules).toHaveBeenCalledWith([
    { domain: "example.com", titleKeyword: "Pull Request", name: "GitHub PRs" },
  ]);
  expect(feedback).toHaveBeenCalledWith("Source mapping rules saved", true);
});
it("removes rules and reports save failures through feedback", async () => {
  target.querySelector<HTMLButtonElement>(".mapping-remove")!.click();
  await settle();
  button("Save rules").click();
  await settle();
  expect(service.setSourceMappingRules).toHaveBeenCalledWith([]);
  vi.mocked(service.setSourceMappingRules).mockRejectedValueOnce(new Error("disk unavailable"));
  button("Add rule").click();
  await settle();
  button("Save rules").click();
  await settle();
  expect(feedback).toHaveBeenCalledWith("disk unavailable", false);
});
