import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
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

function mountPanel(record: ClipboardItem, props: Record<string, unknown> = {}): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(DetailPanel, {
    target,
    props: {
      item: record,
      ...props,
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

/** Drains the microtask queue so the find signal/highlight tick chains settle. */
async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) {
    await Promise.resolve();
    flushSync();
  }
}

const navButton = (bar: Element, title: string) =>
  bar.querySelector<HTMLButtonElement>(`button[title="${title}"]`)!;

// jsdom does not implement scrollIntoView (used by the current-match marker).
beforeAll(() => {
  Element.prototype.scrollIntoView = () => {};
});

/** Types a find query into the bar and waits for the marks to settle. */
async function typeQuery(target: HTMLElement, query: string): Promise<HTMLElement> {
  const input = target.querySelector<HTMLInputElement>(".find-bar input")!;
  input.value = query;
  input.dispatchEvent(new Event("input", { bubbles: true }));
  await settle();
  return target.querySelector(".find-bar")!;
}

describe("DetailPanel find in detail", () => {
  it("opens from the Ctrl+F signal, counts matches, and navigates with wrap-around", async () => {
    const target = mountPanel(item({ textContent: "alpha beta alpha gamma alpha" }), {
      findRequest: { seq: 1 },
    });
    await settle();
    const bar = await typeQuery(target, "alpha");
    expect(target.querySelectorAll("mark[data-find-match]")).toHaveLength(3);
    expect(bar.querySelector(".match-count")?.textContent).toBe("1/3");
    const next = navButton(bar, t("detail.findNext"));
    next.click();
    flushSync();
    expect(bar.querySelector(".match-count")?.textContent).toBe("2/3");
    next.click();
    flushSync();
    expect(bar.querySelector(".match-count")?.textContent).toBe("3/3");
    next.click();
    flushSync();
    expect(bar.querySelector(".match-count")?.textContent).toBe("1/3");
    navButton(bar, t("detail.findPrev")).click();
    flushSync();
    expect(bar.querySelector(".match-count")?.textContent).toBe("3/3");
  });

  it("recounts matches when the query changes", async () => {
    const target = mountPanel(item({ textContent: "alpha beta alpha gamma alpha" }), {
      findRequest: { seq: 1 },
    });
    await settle();
    const input = target.querySelector<HTMLInputElement>(".find-bar input")!;
    input.value = "gamma";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    await settle();
    expect(target.querySelectorAll("mark[data-find-match]")).toHaveLength(1);
    expect(target.querySelector(".find-bar .match-count")?.textContent).toBe("1/1");
  });

  it("closes on Escape without closing the panel and unwraps the marks", async () => {
    const target = mountPanel(item({ textContent: "alpha beta alpha" }), {
      findRequest: { seq: 1 },
    });
    await settle();
    await typeQuery(target, "alpha");
    expect(target.querySelectorAll("mark[data-find-match]")).toHaveLength(2);
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", cancelable: true }));
    await settle();
    expect(target.querySelector(".find-bar")).toBeNull();
    expect(target.querySelectorAll("mark[data-find-match]")).toHaveLength(0);
    expect(target.querySelector(".detail-panel")).not.toBeNull();
  });

  it("ignores the find signal for kinds without a text preview", async () => {
    const target = mountPanel(item({ kind: "image" }), { findRequest: { seq: 1 } });
    await settle();
    expect(target.querySelector(".find-bar")).toBeNull();
    expect(target.querySelectorAll("mark[data-find-match]")).toHaveLength(0);
  });
});

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
