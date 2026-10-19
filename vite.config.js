import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
import adapter from "@sveltejs/adapter-static";
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";
import process from "node:process";
import { execSync } from "node:child_process";
const host = process.env.TAURI_DEV_HOST;

/** The commit being built, with `-dirty` when the tree has changes; null outside git. */
function commit() {
  try {
    const git = (/** @type {string} */ args) =>
      execSync(`git ${args}`, { encoding: "utf8" }).trim();
    const dirty = git("status --porcelain --untracked-files=no") !== "";
    return git("rev-parse --short HEAD") + (dirty ? "-dirty" : "");
  } catch {
    return null;
  }
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [
    // SvelteKit's own options (no svelte.config.js since SvelteKit 3).
    sveltekit({
      preprocess: vitePreprocess(),
      // Tauri has no Node server for SSR, so adapter-static with a fallback page runs the app
      // as an SPA: https://svelte.dev/docs/kit/single-page-apps and
      // https://v2.tauri.app/start/frontend/sveltekit/
      adapter: adapter({ fallback: "index.html" }),
      // The app updates through Tauri's updater, never a redeploy: no hourly version.json poll
      // (SvelteKit 3's default). Kit still checks on every window focus and visibility change,
      // with no option to turn it off; the CSP's connect-src refuses that fetch and kit ignores
      // the failure.
      version: { pollInterval: 0 },
    }),
  ],
  define: {
    __COMMIT__: JSON.stringify(commit()),
  },
  // Unit tests for the UI's own logic (`pnpm test`); runes compile through the plugin. The
  // WebDriver suite's mock GitHub is checked here too, without the suite's dependencies.
  // Playwright's UI flows (`tests/e2e`, `pnpm test:e2e`) stay out.
  test: {
    include: ["src/**/*.test.ts", "scripts/**/*.test.mjs", "e2e/mock/**/*.test.js"],
    environment: "node",
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 1420,
    strictPort: true,
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
