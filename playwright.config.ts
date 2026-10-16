// End-to-end UI flows: the built app (`vite preview`, the same bundle Tauri ships) in WebKit,
// the engine behind WKWebView and WebKitGTK, with Rust faked through Tauri's IPC mocks
// (`tests/e2e/harness`). See CLAUDE.md > Commands.
import { defineConfig, devices } from "@playwright/test";
import process from "node:process";
import { fileURLToPath } from "node:url";

// Not Vite preview's default, so another checkout's preview is never tested by mistake.
const PORT = 4419;
const CI = Boolean(process.env.CI);

export default defineConfig({
  testDir: "tests/e2e",
  outputDir: "test-results",
  fullyParallel: false,
  forbidOnly: CI,
  retries: CI ? 1 : 0,
  // One browser at a time: the suite is light, and laptops stay cool.
  workers: 1,
  timeout: 15_000,
  reporter: CI ? [["list"], ["html", { open: "never" }]] : [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    // The popover's size (tauri.conf.json).
    viewport: { width: 380, height: 560 },
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [
    {
      name: "webkit",
      use: { ...devices["Desktop Safari"], viewport: { width: 380, height: 560 } },
    },
  ],
  webServer: {
    // A production build: no dependency optimizer reloading the page mid-test, and the
    // exact code that ships.
    // `exec`: the server is the process Playwright stops at the end (a pnpm wrapper outlived
    // it and kept the run from exiting).
    command: `vite build && exec vite preview --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}`,
    // The repo root, also for configs elsewhere that reuse this one (`e2e:shots`).
    cwd: fileURLToPath(new URL(".", import.meta.url)),
    // Always this checkout's build: a server already on the port fails the run instead.
    reuseExistingServer: false,
    timeout: 120_000,
  },
});
