import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";
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
  plugins: [sveltekit()],
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
