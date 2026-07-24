// Tauri doesn't have a Node.js server to do proper SSR
// so we use adapter-static with a fallback to index.html to put the site in SPA mode
// See: https://svelte.dev/docs/kit/single-page-apps
// See: https://v2.tauri.app/start/frontend/sveltekit/ for more info
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

// In production Tauri builds the webview loads from tauri://localhost/ but
// bundled frontend assets are served via the asset protocol at
// https://asset.localhost/.  TAURI_DEV_HOST is only set during `tauri dev`
// (where Vite serves everything from localhost), so we switch the assets path
// accordingly.
const isDev = !!process.env.TAURI_DEV_HOST;

/** @type {import('@sveltejs/kit').Config} */
const config = {
  preprocess: vitePreprocess(),
  kit: {
    adapter: adapter({
      fallback: "index.html",
    }),
    paths: {
      assets: isDev ? "" : "https://asset.localhost",
      base: "",
    },
  },
};

export default config;
