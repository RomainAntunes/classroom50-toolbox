// `pnpm site:shots`: the screenshots of the download page and the README, written to
// `site/screenshots/` as WebP (needs `cwebp`: `brew install webp`, `apt install webp`). Same
// server and mocks as the suite; only `showcase.shots.ts` runs.
import { defineConfig } from "@playwright/test";
import base from "../../playwright.config";

export default defineConfig({
  ...base,
  testDir: ".",
  outputDir: "../../test-results",
  testMatch: "**/showcase.shots.ts",
  retries: 0,
  timeout: 60_000,
  reporter: [["list"]],
  use: { ...base.use, trace: "off", screenshot: "off" },
  projects: base.projects?.filter((project) => project.name === "webkit"),
  // The production build only.
  webServer: Array.isArray(base.webServer) ? base.webServer.slice(0, 1) : base.webServer,
});
