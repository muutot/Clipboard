import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { generalSettings, DEFAULT_GENERAL_SETTINGS } from "$lib/services/settings";
import DetailOcrTab from "./DetailOcrTab.svelte";
import type { ClipboardItem } from "$lib/types/clipboard";
const bridge = vi.hoisted(() => ({ write: vi.fn(), invoke: vi.fn() }));
vi.mock("$lib/services/clipboard", async (original) => ({
  ...(await original<typeof import("$lib/services/clipboard")>()),
  writeClipboardText: bridge.write,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: bridge.invoke,
  convertFileSrc: (p: string) => p,
}));
vi.mock("$lib/utils/format", async (original) => ({
  ...(await original<typeof import("$lib/utils/format")>()),
  assetUrl: (path: string) => path,
}));
let target: HTMLDivElement;
let app: ReturnType<typeof mount>;
const update = vi.fn();
const item: ClipboardItem = {
  id: "ocr",
  kind: "image",
  title: "Receipt",
  preview: "",
  sourceApp: "test",
  sourceTone: "neutral",
  sizeLabel: "1KB",
  favorite: false,
  createdAt: 1,
  imageMeta: { width: 1000, height: 500 },
  resourcePath: "/sample.png",
  ocrStatus: "completed",
  ocrText: "Invoice\nTotal",
  ocrBlocks: [
    { text: "Invoice", left: 100, top: 50, width: 200, height: 50, confidence: 0.9 },
    { text: "Total", left: 100, top: 100, width: 100, height: 50, confidence: 0.8 },
  ],
};
async function settle() {
  for (let i = 0; i < 10; i++) {
    await Promise.resolve();
    flushSync();
  }
}
beforeEach(() => {
  generalSettings.set({ ...DEFAULT_GENERAL_SETTINGS, language: "en" });
  bridge.write.mockReset().mockResolvedValue(undefined);
  bridge.invoke.mockReset().mockResolvedValue(true);
  target = document.createElement("div");
  document.body.append(target);
  app = mount(DetailOcrTab, { target, props: { item, query: "invoice", onocrupdate: update } });
  flushSync();
});
afterEach(async () => {
  await unmount(app);
  target.remove();
  vi.clearAllMocks();
});
it("links image-region selection with text selection and copies in reading order", async () => {
  const regions = [...target.querySelectorAll<HTMLButtonElement>(".ocr-box")];
  expect(regions[0].classList.contains("matched")).toBe(true);
  expect(regions[0].style.left).toBe("10%");
  regions[1].click();
  regions[0].click();
  flushSync();
  expect(target.querySelectorAll("input:checked")).toHaveLength(2);
  const copy = [...target.querySelectorAll<HTMLButtonElement>("button")].find((b) =>
    b.textContent?.includes("Copy selected"),
  )!;
  copy.click();
  await settle();
  expect(bridge.write).toHaveBeenCalledWith("Invoice\nTotal");
});
it("clears block geometry when regeneration is queued", async () => {
  target.querySelector<HTMLButtonElement>(".ocr-regenerate-btn")!.click();
  await settle();
  expect(update).toHaveBeenCalledWith(
    "ocr",
    expect.objectContaining({ ocrStatus: "pending", ocrBlocks: undefined }),
  );
});
