// End-to-end UI flows: the built app (`vite preview`, the same bundle Tauri ships) in WebKit,
// the engine behind WKWebView and WebKitGTK, with Rust faked through Tauri's IPC mocks
// (`tests/e2e/harness`). See CLAUDE.md > Commands.
import { defineConfig, devices } from "@playwright/test";
import process from "node:process";

const PORT = 4173;
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
    command: `pnpm build && pnpm preview --port ${PORT} --strictPort --host 127.0.0.1`,
    url: `http://127.0.0.1:${PORT}`,
    reuseExistingServer: !CI,
    timeout: 120_000,
  },
});
