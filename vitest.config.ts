import { sveltekit } from "@sveltejs/kit/vite";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [sveltekit()],
  // Svelte ships separate client/server builds; resolving the browser condition
  // is what makes `mount()` and rune ownership work under jsdom.
  resolve: { conditions: ["browser"] },
  test: {
    environment: "jsdom",
    include: ["src/**/*.test.ts"],
  },
});
