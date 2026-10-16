// The WebDriver smoke suite: drives the real app (a debug build, so the dev write guard is on)
// through tauri-driver and WebKitWebDriver, Linux only (macOS has no WKWebView driver). The
// launcher prepares a fresh home and a fixture clone, starts the mock GitHub and tauri-driver,
// and points the app at them through the `testenv.rs` hooks.

import { spawn } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { createConnection } from "node:net";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { start } from "./mock/github.js";
import { buildWorld } from "./mock/world.js";
import { IDENTIFIER, prepare } from "./support/fixture.js";

const here = fileURLToPath(new URL(".", import.meta.url));
const application =
  process.env.C50_E2E_APP ??
  join(here, "..", "src-tauri", "target", "debug", "classroom50-toolbox");
const artifacts = process.env.C50_E2E_ARTIFACTS ?? join(here, "artifacts");
/** tauri-driver's default port. */
const DRIVER_PORT = 4444;

/** @type {import("node:child_process").ChildProcess | undefined} */
let driver;
/** @type {Awaited<ReturnType<typeof start>> | undefined} */
let mock;
/** @type {ReturnType<typeof prepare> | undefined} */
let fixture;

/** Resolves once something accepts connections on `port`. */
async function listening(port, timeout = 30_000) {
  const until = Date.now() + timeout;
  while (Date.now() < until) {
    const open = await new Promise((resolve) => {
      const socket = createConnection({ host: "127.0.0.1", port });
      socket.once("connect", () => {
        socket.end();
        resolve(true);
      });
      socket.once("error", () => resolve(false));
    });
    if (open) return;
    await new Promise((resolve) => setTimeout(resolve, 250));
  }
  throw new Error(`nothing listens on port ${port} after ${timeout} ms`);
}

/** Keeps what explains a failure: the app's log, the requests the mock saw, gh's calls. */
function saveArtifacts() {
  if (!fixture) return;
  mkdirSync(artifacts, { recursive: true });
  const logs = join(fixture.env.XDG_DATA_HOME, IDENTIFIER, "logs");
  if (existsSync(logs)) cpSync(logs, join(artifacts, "app-logs"), { recursive: true });
  if (mock)
    writeFileSync(join(artifacts, "mock-requests.json"), JSON.stringify(mock.requests, null, 2));
  const ghLog = join(fixture.root, "gh-calls.log");
  if (existsSync(ghLog)) cpSync(ghLog, join(artifacts, "gh-calls.log"));
}

export const config = {
  runner: "local",
  hostname: "127.0.0.1",
  port: DRIVER_PORT,
  specs: ["./specs/**/*.e2e.js"],
  // One app at a time: the single-instance plugin would turn a second one away.
  maxInstances: 1,
  capabilities: [
    {
      // tauri-driver turns this into WebKitWebDriver's `webkitgtk:browserOptions`.
      "tauri:options": { application },
    },
  ],
  logLevel: "warn",
  outputDir: join(artifacts, "wdio"),
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: { ui: "bdd", timeout: 120_000 },
  waitforTimeout: 30_000,
  connectionRetryTimeout: 120_000,
  connectionRetryCount: 1,

  async onPrepare() {
    if (!existsSync(application)) {
      throw new Error(`no app at ${application}: run pnpm tauri build --debug --no-bundle first`);
    }
    fixture = prepare();
    mock = await start(buildWorld({ heads: fixture.heads }));
    const ghLog = join(fixture.root, "gh-calls.log");
    // Read by the specs, which run in worker processes started after this.
    process.env.C50_E2E_CLONE = fixture.clone;
    process.env.C50_E2E_GH_LOG = ghLog;
    process.env.C50_E2E_MOCK = mock.url;

    const env = { ...process.env, ...fixture.env };
    // gh would take these over its own login, and the app would say so.
    delete env.GH_TOKEN;
    delete env.GITHUB_TOKEN;
    Object.assign(env, {
      C50_TEST_GITHUB: mock.url,
      C50_TEST_BIN: join(here, "bin"),
      C50_TEST_OPEN: "1",
      C50_FAKE_GH_LOG: ghLog,
      // WebKitGTK under Xvfb: no GPU, no DMA-BUF renderer.
      WEBKIT_DISABLE_DMABUF_RENDERER: "1",
      LIBGL_ALWAYS_SOFTWARE: "1",
      GDK_BACKEND: "x11",
      // No accessibility bus in CI: don't wait for one.
      NO_AT_BRIDGE: "1",
    });
    // tauri-driver starts WebKitWebDriver, which starts the app: both pass the environment on.
    driver = spawn(process.env.TAURI_DRIVER ?? "tauri-driver", [], {
      env,
      stdio: ["ignore", "inherit", "inherit"],
    });
    driver.once("exit", (code, signal) => {
      if (driver) console.error(`tauri-driver exited early (${code ?? signal})`);
    });
    await listening(DRIVER_PORT);
  },

  async afterTest(test, _context, { passed }) {
    if (passed) return;
    mkdirSync(artifacts, { recursive: true });
    const name = `${test.parent} ${test.title}`.replace(/[^a-z0-9]+/gi, "-").toLowerCase();
    await browser.saveScreenshot(join(artifacts, `${name}.png`));
  },

  async onComplete() {
    saveArtifacts();
    const running = driver;
    driver = undefined;
    running?.kill();
    await mock?.close();
    // What explains a failure is in the artifacts; `C50_E2E_KEEP=1` keeps the whole run.
    if (fixture && !process.env.C50_E2E_KEEP)
      rmSync(fixture.root, { recursive: true, force: true });
  },
};
