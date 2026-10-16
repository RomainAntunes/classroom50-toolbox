// `pnpm e2e:shots`: screenshots of the main screens in every skin, light and dark, written
// to `e2e-shots/` (gitignored) for reviewing a UI change. `SHOTS_LOCALE=de` picks another
// catalog. Same server and mocks as the suite; only `*.shots.ts` runs.
import { defineConfig } from "@playwright/test";
import base from "../../playwright.config";

export default defineConfig({
  ...base,
  testDir: ".",
  outputDir: "../../test-results",
  testMatch: "**/*.shots.ts",
  retries: 0,
  reporter: [["list"]],
  use: { ...base.use, trace: "off", screenshot: "off" },
  // The production build only; the suite's dev-build project matches its own specs.
  projects: base.projects?.filter((project) => project.name === "webkit"),
});
