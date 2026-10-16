// The fake Rust side only exists in the init script the tests inject: the bundle Tauri
// ships (`build/`, which `vite preview` serves to these tests) must hold none of it.
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { expect, test } from "@playwright/test";

const BUILD = new URL("../../build", import.meta.url).pathname;
const MARKERS = ["__E2E__", "__E2E_HARNESS__", "mockIPC", "mockWindows", "shouldMockEvents"];

function files(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) =>
    entry.isDirectory() ? files(join(dir, entry.name)) : [join(dir, entry.name)],
  );
}

test("the production bundle holds no test mocks", () => {
  const code = files(BUILD).filter((file) => /\.(js|html)$/.test(file));
  expect(code.length).toBeGreaterThan(0);
  const leaks = code.flatMap((file) => {
    const text = readFileSync(file, "utf8");
    return MARKERS.filter((marker) => text.includes(marker)).map((m) => `${file}: ${m}`);
  });
  expect(leaks).toEqual([]);
});
