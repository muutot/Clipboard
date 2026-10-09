import { flushSync, mount, unmount } from "svelte";
import { fromStore, writable } from "svelte/store";
import { afterEach, expect, it, vi } from "vitest";
import type { ClipboardItem } from "$lib/types/clipboard";
import DetailFilePreview from "./DetailFilePreview.svelte";
import { loadFilePreview } from "$lib/utils/file-preview";

vi.mock("$lib/services/settings", async () => {
  const { writable } = await import("svelte/store");
  return { generalSettings: writable({ colorIcons: false }) };
});

vi.mock("$lib/services/runtime", () => ({
  isTauriRuntime: () => true,
  invokeTauri: async () => null,
}));
vi.mock("$lib/utils/format", async (original) => ({
  ...(await original<object>()),
  assetUrl: (path: string) => path,
}));

const limit = 512 * 1024;
const item: ClipboardItem = {
  id: "file",
  kind: "file",
  title: "fixture.txt",
  preview: "",
  sourceApp: "test",
  sourceTone: "neutral",
  sizeLabel: "",
  createdAt: 0,
  favorite: false,
  mimeType: "text/plain",
  resourcePath: "/fixture.txt",
  fileMeta: [{ name: "fixture.txt", size: 1, sizeBytes: 1 }],
};
const cleanups: (() => Promise<void>)[] = [];
afterEach(async () => {
  for (const cleanup of cleanups.splice(0)) await cleanup();
  vi.unstubAllGlobals();
});
async function settle() {
  for (let i = 0; i < 20; i++) {
    await Promise.resolve();
    flushSync();
  }
}
function render() {
  const store = writable(item);
  const state = fromStore(store);
  const target = document.createElement("div");
  document.body.append(target);
  const app = mount(DetailFilePreview, {
    target,
    props: {
      get item() {
        return state.current;
      },
    },
  });
  const close = async () => {
    await unmount(app);
    target.remove();
  };
  cleanups.push(close);
  flushSync();
  return { store, target, close };
}

it("bounds consumption and cancels the remainder when a server ignores Range", async () => {
  const cancel = vi.fn();
  let reads = 0;
  const body = new ReadableStream<Uint8Array>(
    {
      pull(controller) {
        reads++;
        controller.enqueue(new Uint8Array(64 * 1024).fill(65));
        if (reads === 12) controller.close();
      },
      cancel,
    },
    { highWaterMark: 0 },
  );
  const fetcher = vi.fn().mockResolvedValue(new Response(body));
  vi.stubGlobal("fetch", fetcher);
  const page = render();
  await vi.waitFor(() => expect(page.target.querySelector("pre")?.textContent).toHaveLength(limit));
  expect(reads).toBe(9);
  expect(cancel).toHaveBeenCalledOnce();
  expect(fetcher.mock.calls[0][1].headers).toEqual({ Range: `bytes=0-${limit}` });
});

it("keeps a pending preview during unrelated edits, aborts on path change and unmount", async () => {
  const pending: { signal: AbortSignal; finish: (response: Response) => void }[] = [];
  vi.stubGlobal(
    "fetch",
    vi.fn(
      (_url, options) =>
        new Promise<Response>((finish) => {
          pending.push({ signal: options?.signal, finish });
        }),
    ),
  );
  const page = render();
  page.store.update((value) => ({ ...value, title: "renamed" }));
  await settle();
  expect(pending).toHaveLength(1);
  expect(pending[0].signal?.aborted).toBe(false);
  page.store.update((value) => ({ ...value, resourcePath: "/second.txt" }));
  await settle();
  expect(pending[0].signal.aborted).toBe(true);
  expect(pending).toHaveLength(2);
  pending[0].finish(new Response("stale"));
  await settle();
  expect(page.target.textContent).not.toContain("stale");
  await page.close();
  cleanups.pop();
  expect(pending[1].signal.aborted).toBe(true);
  pending[1].finish(new Response("unmounted"));
  await settle();
});

it.each([0, limit, limit + 1])(
  "distinguishes a %s-byte partial response from truncation",
  async (size) => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValue(
          size === 0
            ? new Response(null, { status: 416, headers: { "Content-Range": "bytes */0" } })
            : new Response(new Uint8Array(size).fill(65), { status: 206 }),
        ),
    );
    const result = await loadFilePreview("/fixture", new AbortController().signal);
    expect(result.text).toHaveLength(Math.min(size, limit));
    expect(result.truncated).toBe(size > limit);
  },
);

it("cancels a pending body read when the preview is aborted", async () => {
  const cancel = vi.fn();
  const controller = new AbortController();
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(new Response(new ReadableStream({ cancel }))));
  const result = loadFilePreview("/fixture", controller.signal);
  const rejected = expect(result).rejects.toMatchObject({ name: "AbortError" });
  await settle();
  controller.abort();
  await rejected;
  expect(cancel).toHaveBeenCalledOnce();
});
