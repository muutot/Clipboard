import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import DetailPanel from "./DetailPanel.svelte";
import { t } from "$lib/i18n";
import type { ClipboardItem } from "$lib/types/clipboard";
import Probe, { type DetailProbeState } from "./DetailPanel.probe.svelte";

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

describe("DetailPanel pending saves", () => {
  it.each(["different-record", "cancel-reopen", "cancel-reopen-same", "draft-edited", "unchanged"])(
    "retains a newer draft while an old save finishes (%s)",
    async (change) => {
      const target = document.createElement("div");
      document.body.append(target);
      const out: DetailProbeState = {};
      let finish!: (ok: boolean) => void;
      const save = vi.fn(
        () =>
          new Promise<boolean>((resolve) => {
            finish = resolve;
          }),
      );
      const original = item({ id: "old", textContent: "original body" });
      const app = mount(Probe, { target, props: { out, initial: original, save } });
      cleanups.push(async () => {
        await unmount(app);
        target.remove();
      });
      flushSync();
      const begin = () => {
        [...target.querySelectorAll<HTMLButtonElement>(".detail-actions button")]
          .find((button) => button.textContent?.trim() === t("edit.edit"))!
          .click();
        flushSync();
      };
      begin();
      target.querySelector<HTMLButtonElement>(".edit-save")!.click();
      expect(save).toHaveBeenCalledWith("old", "original body");
      if (change === "different-record") {
        out.setItem!(item({ id: "new", textContent: "new body" }));
        flushSync();
        begin();
      } else if (change.startsWith("cancel-reopen")) {
        target.querySelector<HTMLButtonElement>(".edit-cancel")!.click();
        flushSync();
        begin();
      }
      const expectedDraft =
        change === "cancel-reopen-same" || change === "unchanged"
          ? "original body"
          : "UNSAVED NEW DRAFT";
      const draft = target.querySelector<HTMLElement>(".code-input")!;
      draft.textContent = expectedDraft;
      draft.dispatchEvent(new Event("input", { bubbles: true }));
      flushSync();
      finish(true);
      for (let i = 0; i < 12; i++) {
        await Promise.resolve();
        flushSync();
      }
      if (change === "unchanged") expect(target.querySelector(".code-input")).toBeNull();
      else
        expect(target.querySelector<HTMLElement>(".code-input")?.textContent).toBe(expectedDraft);
    },
  );
});
