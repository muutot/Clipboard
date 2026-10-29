import { expect, it, vi } from "vitest";
import { ensureSourceMappingRules } from "$lib/services/source-mapping.svelte";
import { load } from "./+layout";

vi.mock("$lib/services/settings", () => ({
  generalSettings: {
    initialize: vi.fn(async () => {}),
    subscribe: (listener: (settings: { language: string }) => void) => {
      listener({ language: "en" });
      return () => {};
    },
  },
}));
vi.mock("$lib/i18n", () => ({ loadOsLocale: vi.fn(async () => {}), setLocale: vi.fn() }));
vi.mock("$lib/services/source-mapping.svelte", () => ({ ensureSourceMappingRules: vi.fn() }));

it("waits for source mapping hydration before allowing the first layout render", async () => {
  let finish!: () => void;
  vi.mocked(ensureSourceMappingRules).mockReturnValueOnce(
    new Promise<void>((resolve) => (finish = resolve)),
  );
  let rendered = false;
  const loading = load().then(() => (rendered = true));
  await Promise.resolve();
  expect(ensureSourceMappingRules).toHaveBeenCalledTimes(1);
  expect(rendered).toBe(false);
  finish();
  await loading;
  expect(rendered).toBe(true);
});
