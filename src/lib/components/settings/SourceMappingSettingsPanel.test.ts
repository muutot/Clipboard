import { mount, unmount, flushSync } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import Panel from "./SourceMappingSettingsPanel.svelte";
import { locale } from "$lib/i18n";
import * as service from "$lib/services/clipboard";
import {
  destroySourceMappingRules,
  resolveWebSourceName,
} from "$lib/services/source-mapping.svelte";

vi.mock("$lib/services/runtime", () => ({ isTauriRuntime: () => true }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
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
  destroySourceMappingRules();
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
  await vi.waitFor(() => expect(feedback).toHaveBeenCalledWith("Source mapping rules saved", true));
  expect(service.setSourceMappingRules).toHaveBeenCalledWith([
    { domain: "example.com", titleKeyword: "Pull Request", name: "GitHub PRs" },
  ]);
});
it("removes rules and reports save failures through feedback", async () => {
  target.querySelector<HTMLButtonElement>(".mapping-remove")!.click();
  await settle();
  button("Save rules").click();
  await vi.waitFor(() => expect(feedback).toHaveBeenCalledWith("Source mapping rules saved", true));
  expect(service.setSourceMappingRules).toHaveBeenCalledWith([]);
  vi.mocked(service.setSourceMappingRules).mockRejectedValueOnce(new Error("disk unavailable"));
  button("Add rule").click();
  await settle();
  button("Save rules").click();
  await vi.waitFor(() => expect(feedback).toHaveBeenCalledWith("disk unavailable", false));
});

it("refreshes names from the backend when an older save response arrives late", async () => {
  let finishSave!: (rules: service.SourceMappingRule[]) => void;
  const saved = new Promise<service.SourceMappingRule[]>((resolve) => (finishSave = resolve));
  vi.mocked(service.setSourceMappingRules).mockReturnValueOnce(saved);
  button("Save rules").click();
  await settle();
  const latest = { ...rule, name: "Latest rules from another window" };
  vi.mocked(service.getSourceMappingRules).mockResolvedValue([latest]);
  finishSave([rule]);
  await vi.waitFor(() => expect(feedback).toHaveBeenCalledWith("Source mapping rules saved", true));
  expect(resolveWebSourceName({ sourceApp: "Chrome", webSource: { domain: "github.com" } })).toBe(
    latest.name,
  );
});
