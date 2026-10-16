// Bundles `browser.ts` (with Tauri's mocks) into one classic script for `addInitScript`,
// once per worker. Vite's own config is skipped: this is a plain library build.
import { fileURLToPath } from "node:url";
import { build, type Rolldown } from "vite";

let cached: Promise<string> | undefined;

export function harnessScript(): Promise<string> {
  cached ??= build({
    configFile: false,
    logLevel: "warn",
    build: {
      write: false,
      minify: false,
      lib: {
        entry: fileURLToPath(new URL("./browser.ts", import.meta.url)),
        formats: ["iife"],
        name: "__E2E_HARNESS__",
      },
    },
  }).then((result) => {
    const outputs = (Array.isArray(result) ? result : [result]) as Rolldown.RolldownOutput[];
    return outputs[0].output[0].code;
  });
  return cached;
}
