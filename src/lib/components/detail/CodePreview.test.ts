import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, it } from "vitest";
import { getLocale, setLocale, t } from "$lib/i18n";
import { FILE_PREVIEW_LIMIT } from "$lib/utils/content/file-preview";
import CodePreview from "./CodePreview.svelte";

const initialLocale = getLocale();
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  setLocale(initialLocale);
});

function render(content: string) {
  const store = writable(content);
  const state = fromStore(store);
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(CodePreview, {
    target,
    props: {
      get content() {
        return state.current;
      },
      language: "JavaScript",
    },
  });
  cleanups.push(async () => {
    await unmount(app);
    target.remove();
  });
  flushSync();
  return { target, store };
}

const renderedCode = (target: HTMLElement) =>
  [...target.querySelectorAll(".code-line")]
    .map((line) => {
      const clone = line.cloneNode(true) as HTMLElement;
      clone.querySelector(".line-number")?.remove();
      return clone.textContent ?? "";
    })
    .join("\n");

it("discloses truncated code and updates the notice with the locale", () => {
  setLocale("en");
  const source = "const first = 1;\n//" + "a".repeat(FILE_PREVIEW_LIMIT) + "\nTAIL_SENTINEL";
  const { target } = render(source);
  expect(renderedCode(target)).toBe("const first = 1;");
  expect(target.textContent).not.toContain("TAIL_SENTINEL");
  expect(target.querySelector('[role="status"]')?.textContent).toBe(
    t("detail.codePreviewTruncated"),
  );
  const englishNotice = target.querySelector('[role="status"]')?.textContent;
  expect(englishNotice).not.toBe("detail.codePreviewTruncated");
  setLocale("zh-CN");
  flushSync();
  expect(target.querySelector('[role="status"]')?.textContent).toBe(
    t("detail.codePreviewTruncated"),
  );
  expect(target.querySelector('[role="status"]')?.textContent).not.toBe(englishNotice);
});

it.each([0, FILE_PREVIEW_LIMIT - 1, FILE_PREVIEW_LIMIT])(
  "renders all %s characters without a truncation notice",
  (length) => {
    const source = length === 0 ? "" : "//" + "a".repeat(length - 2);
    const { target } = render(source);
    expect(renderedCode(target)).toBe(source);
    expect(target.querySelector('[role="status"]')).toBeNull();
  },
);

it("removes the truncation notice when content changes to a complete preview", () => {
  const { target, store } = render("//" + "a".repeat(FILE_PREVIEW_LIMIT));
  expect(renderedCode(target)).toHaveLength(FILE_PREVIEW_LIMIT);
  expect(target.querySelector('[role="status"]')).not.toBeNull();
  store.set("const complete = 1;");
  flushSync();
  expect(renderedCode(target)).toBe("const complete = 1;");
  expect(target.querySelector('[role="status"]')).toBeNull();
});
