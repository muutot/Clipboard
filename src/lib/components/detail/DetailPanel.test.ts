import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import DetailPanel from "./DetailPanel.svelte";
import { t } from "$lib/i18n";
import type { ClipboardItem } from "$lib/types/clipboard";

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ label: "main" }),
}));

const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.unstubAllGlobals();
});

function item(overrides: Partial<ClipboardItem>): ClipboardItem {
  return {
    id: "record",
    kind: "text",
    title: "record",
    preview: "",
    sourceApp: "test",
    sourceTone: "neutral",
    sizeLabel: "1 B",
    createdAt: 1,
    favorite: false,
    ...overrides,
  };
}

function mountPanel(record: ClipboardItem): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(DetailPanel, {
    target,
    props: {
      item: record,
      onclose: () => {},
      oncopy: () => {},
      onedit: () => {},
      onsaveedit: async () => true,
      onrenametitle: () => {},
      onplainpaste: () => {},
      onformatpaste: () => {},
      oncleanpaste: () => {},
      onduplicate: () => {},
      onsaveasnew: () => {},
      oncopyfilename: () => {},
      oncopyPath: () => {},
      onsavetags: () => {},
      onocrupdate: () => {},
    },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return target;
}

const tabLabels = (target: HTMLElement) =>
  [...target.querySelectorAll<HTMLButtonElement>(".detail-tabs button")].map((button) =>
    button.textContent?.trim(),
  );

describe("DetailPanel OCR tab applicability", () => {
  it("hides the OCR tab for text records", () => {
    const target = mountPanel(item({ kind: "text", textContent: "hello" }));
    expect(tabLabels(target)).not.toContain(t("detail.ocr"));
  });

  it("hides the OCR tab for file records", () => {
    const target = mountPanel(item({ kind: "file", fileName: "clip.mp4" }));
    expect(tabLabels(target)).not.toContain(t("detail.ocr"));
  });

  it("keeps the OCR tab for image records", () => {
    const target = mountPanel(item({ kind: "image" }));
    expect(tabLabels(target)).toContain(t("detail.ocr"));
  });
});
